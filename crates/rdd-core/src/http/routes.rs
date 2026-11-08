//! `RouteDef` y `HttpApp`: las rutas como datos, con los middlewares ya compuestos (§4.4).

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use crate::di::Container;
use crate::error::{Error, Problem};
use crate::http::extract::PathParams;
use crate::http::response::{AllowedMethods, error_response_for, problem_response};
use crate::http::types::{
    BoxHandler, BoxedMiddleware, CoreHandler, Method, Request, Response, StatusCode, compose,
    materialize, reopen,
};

/// Una ruta declarativa: método, ruta con sintaxis `/:nombre` y handler completo.
///
/// El handler llega a la plataforma ya envuelto por toda su cadena de middlewares
/// (RP-005) y nunca devuelve `Err`: los errores están materializados como
/// Problem Details (RP-004). Es también la forma que devuelve el `routes()`
/// generado por `#[controller]` (RF-056).
#[derive(Clone)]
pub struct RouteDef {
    /// Método HTTP.
    pub method: Method,
    /// Ruta, siempre con sintaxis `/:nombre` (RP-003).
    pub path: String,
    /// Handler envuelto por su cadena de middlewares.
    pub handler: BoxHandler,
}

impl RouteDef {
    /// Crea una ruta a partir de un handler ya cerrado.
    pub fn new(method: Method, path: &str, handler: BoxHandler) -> Self {
        Self {
            method,
            path: normalize_path(path),
            handler,
        }
    }

    /// Cierra una cadena de núcleo como handler de plataforma.
    pub fn from_chain(method: Method, path: &str, chain: CoreHandler) -> Self {
        Self::new(method, path, materialize(chain))
    }

    /// Ruta ya materializada envuelta por middlewares adicionales, por fuera.
    ///
    /// Es el mecanismo del orden de §11.3: `#[controller]` entrega `RouteDef`
    /// cerradas con sus middlewares de ruta (RF-056) y el ensamblado les añade
    /// por fuera los de controller, módulo y globales.
    pub fn wrapped_with(&self, outer: &[BoxedMiddleware]) -> RouteDef {
        let chain = compose(outer, reopen(Arc::clone(&self.handler)));
        RouteDef::from_chain(self.method.clone(), &self.path, chain)
    }

    /// Ruta normalizada sin barra final redundante (RF-018).
    pub fn normalized_path(&self) -> String {
        normalize_path(&self.path)
    }
}

/// Normaliza la barra final: `/tasks/` y `/tasks` son la misma ruta (RF-018, RP-008).
pub fn normalize_path(path: &str) -> String {
    let trimmed = if path.len() > 1 && path.ends_with('/') {
        path.trim_end_matches('/')
    } else {
        path
    };
    if trimmed.is_empty() { "/" } else { trimmed }.to_owned()
}

/// Límites que la plataforma debe aplicar (§4.4, RF-017, RP-009).
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Tamaño máximo del cuerpo de petición (RF-017).
    pub body_limit_bytes: usize,
    /// Plazo de drenaje al apagar (RP-009).
    pub shutdown_grace: std::time::Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            body_limit_bytes: crate::app::DEFAULT_BODY_LIMIT,
            shutdown_grace: std::time::Duration::from_secs(30),
        }
    }
}

/// La aplicación HTTP como datos: rutas, fallback y límites (§4.4).
///
/// El núcleo compone todo lo semántico; la plataforma solo sabe servirlo.
pub struct HttpApp {
    /// Rutas montadas, con su cadena de middlewares ya compuesta.
    pub routes: Vec<RouteDef>,
    /// Decide 404 o 405; también envuelto por los middlewares globales (RF-081).
    pub fallback: BoxHandler,
    /// Límites aplicables por la plataforma (RP-006, RP-009).
    pub limits: Limits,
    router: OnceLock<RouteTable>,
}

impl HttpApp {
    /// Compone rutas, fallback y contenedor en la app que recibe la plataforma.
    ///
    /// El orden final de cada ruta es global → módulo → controlador → ruta
    /// (RF-080); las `RouteDef` que entran ya traen compuestos los middlewares de
    /// controller y de ruta (los genera `#[controller]`, RF-056) y los de módulo
    /// (los añade [`crate::module`]). El contenedor se inyecta antes de la cadena
    /// completa para que cualquier middleware pueda resolver servicios (RF-022).
    pub(crate) fn assemble(
        routes: Vec<RouteDef>,
        globals: Vec<BoxedMiddleware>,
        container: Arc<Container>,
        limits: Limits,
    ) -> Self {
        let routes: Vec<RouteDef> = routes
            .into_iter()
            .map(|def| {
                let closed = close(Arc::clone(&container), &globals, reopen(def.handler));
                RouteDef::from_chain(def.method, &def.path, closed)
            })
            .collect();

        let fallback_chain: CoreHandler =
            Arc::new(|req: Request| Box::pin(async move { fallback_response(req) }));
        let fallback = close(Arc::clone(&container), &globals, fallback_chain);

        Self {
            routes,
            fallback: materialize(fallback),
            limits,
            router: OnceLock::new(),
        }
    }

    /// App vacía: responde 404 a todo (RF-010).
    pub fn empty(container: Arc<Container>, limits: Limits) -> Self {
        Self::assemble(Vec::new(), Vec::new(), container, limits)
    }

    /// Tabla de rutas del núcleo, construida una sola vez (V8: `matchit`).
    fn router(&self) -> &RouteTable {
        self.router.get_or_init(|| RouteTable::build(&self.routes))
    }

    /// Enrutado y ejecución sin servidor: plataforma de referencia en proceso
    /// (RP-014, RF-160). Lo usan `TestApp` y, si le conviene, `rdd-hyper`.
    pub async fn dispatch(&self, req: Request) -> Response {
        let path = normalize_path(req.uri().path());
        let method = req.method().clone();

        // RP-006: el límite se comprueba antes de invocar al handler.
        if let Some(resp) = self.check_body(&req) {
            return resp;
        }

        match self.lookup(&path, &method) {
            Look::Hit(handler, params) => {
                let req = with_extensions(req, params, None);
                invoke_guarded(handler, req, &path).await
            }
            Look::MethodNotAllowed(allowed) => {
                let req = with_extensions(req, Vec::new(), Some(AllowedMethods(allowed)));
                invoke_guarded(Arc::clone(&self.fallback), req, &path).await
            }
            Look::Missing => {
                let req = with_extensions(req, Vec::new(), None);
                invoke_guarded(Arc::clone(&self.fallback), req, &path).await
            }
        }
    }

    /// RP-006: 413 en Problem Details cuando el cuerpo supera el límite.
    ///
    /// Es público porque lo necesitan también las plataformas: el límite lo aplica
    /// quien lee el cuerpo, pero el cuerpo de la respuesta lo decide el núcleo.
    pub fn payload_too_large(&self, req: &Request) -> Option<Response> {
        if req.body().len() > self.limits.body_limit_bytes {
            let problem = Problem::payload_too_large(
                self.limits.body_limit_bytes,
                // La misma grafía normalizada que usan el fallback y las
                // plataformas, para que `instance` no dependa de la barra final.
                &normalize_path(req.uri().path()),
            );
            return Some(problem_response(
                StatusCode::PAYLOAD_TOO_LARGE,
                &problem,
                None,
            ));
        }
        None
    }

    /// RP-006: el límite se comprueba antes de invocar al handler.
    fn check_body(&self, req: &Request) -> Option<Response> {
        self.payload_too_large(req)
    }

    /// Búsqueda en la tabla propia del núcleo.
    fn lookup(&self, path: &str, method: &Method) -> Look {
        match self.router().at(path) {
            Some((handlers, params)) => match handlers.get(method) {
                Some(handler) => Look::Hit(Arc::clone(handler), params),
                None => Look::MethodNotAllowed(handlers.keys().cloned().collect()),
            },
            // NotFound y rutas malformadas acaban en 404; nunca en panic.
            None => Look::Missing,
        }
    }

    /// Rutas montadas, para el registro de arranque (RN-042).
    pub fn mounted(&self) -> Vec<(Method, String)> {
        self.router().routes()
    }

    /// Lista legible `VERBO ruta`, para logs y para `rdd check`.
    pub fn route_listing(&self) -> Vec<String> {
        self.mounted()
            .into_iter()
            .map(|(m, p)| format!("{} {}", m.as_str(), p))
            .collect()
    }
}

/// Añade los middlewares globales por fuera y el contenedor antes de todo.
fn close(
    container: Arc<Container>,
    globals: &[BoxedMiddleware],
    chain: CoreHandler,
) -> CoreHandler {
    let with_globals = compose(globals, chain);
    Arc::new(move |mut req: Request| {
        let inner = Arc::clone(&with_globals);
        let container = Arc::clone(&container);
        Box::pin(async move {
            req.extensions_mut().insert(container);
            inner(req).await
        })
    })
}

/// Añade los parámetros de ruta y `AllowedMethods` a las extensiones (RP-003, RP-007).
///
/// Lo usan `dispatch` y las plataformas: cada una captura los parámetros con su
/// propio router, pero el formato que ven los extractores lo fija el núcleo.
pub fn with_extensions(
    mut req: Request,
    params: Vec<(String, String)>,
    allowed: Option<AllowedMethods>,
) -> Request {
    if !params.is_empty() {
        req.extensions_mut().insert(PathParams::new(params));
    }
    if let Some(allowed) = allowed {
        req.extensions_mut().insert(allowed);
    }
    req
}

/// RP-007: el fallback decide entre 404 y 405 según `AllowedMethods`.
fn fallback_response(req: Request) -> crate::Result<Response> {
    let instance = normalize_path(req.uri().path());
    let allowed = req
        .extensions()
        .get::<AllowedMethods>()
        .filter(|a| !a.0.is_empty())
        .cloned();

    Ok(
        match allowed.as_ref().and_then(AllowedMethods::allow_header) {
            Some(allow) => {
                let problem = Problem {
                    r#type: "about:blank".to_owned(),
                    title: "Method Not Allowed".to_owned(),
                    status: StatusCode::METHOD_NOT_ALLOWED.as_u16(),
                    detail: format!(
                        "el método {} no está permitido en esta ruta",
                        req.method().as_str()
                    ),
                    instance,
                };
                problem_response(StatusCode::METHOD_NOT_ALLOWED, &problem, Some(allow))
            }
            None => {
                let problem = Problem {
                    r#type: "about:blank".to_owned(),
                    title: "Not Found".to_owned(),
                    status: StatusCode::NOT_FOUND.as_u16(),
                    detail: format!("la ruta {instance} no existe"),
                    instance,
                };
                problem_response(StatusCode::NOT_FOUND, &problem, None)
            }
        },
    )
}

/// Resultado de la búsqueda de ruta en `dispatch`.
enum Look {
    /// Coincide método y ruta: handler más parámetros capturados.
    Hit(BoxHandler, Vec<(String, String)>),
    /// La ruta existe con otros métodos (RP-007).
    MethodNotAllowed(Vec<Method>),
    /// La ruta no existe.
    Missing,
}

/// Ejecuta un handler aislándolo en una tarea de Tokio.
///
/// Un `panic` mata la tarea, no el servidor: `JoinError` se convierte en 500 y
/// la escucha sigue viva (RF-073, RP-010, C-07).
/// Captura de pánicos por handler (§4.4, RF-073): cada invocación corre en su
/// propia tarea, así que un `panic!` del dominio devuelve 500 y no tumba ni el
/// servidor ni la conexión. Lo reutilizan las plataformas.
pub async fn invoke_guarded(handler: BoxHandler, req: Request, path: &str) -> Response {
    let task = tokio::spawn(async move { (handler)(req).await });
    match task.await {
        Ok(resp) => resp,
        Err(err) => {
            let reason = if err.is_panic() {
                "el handler entró en pánico"
            } else {
                "la tarea del handler fue cancelada"
            };
            tracing::error!(path, error = %err, "fallo del handler aislado");
            error_response_for(&Error::internal(anyhow::anyhow!("{reason}")), path)
        }
    }
}

type MethodMap = HashMap<Method, BoxHandler>;

/// Tabla de rutas del núcleo, construida con `matchit` (V8).
///
/// `matchit` habla `{nombre}`, así que [`to_route_syntax`] traduce `/:nombre`
/// antes de insertar (RP-003). Como `matchit` 0.9 no permite recorrer el árbol,
/// la tabla guarda su propio inventario para [`HttpApp::mounted`].
pub(crate) struct RouteTable {
    inner: matchit::Router<MethodMap>,
    listing: Vec<(Method, String)>,
}

impl RouteTable {
    fn build(routes: &[RouteDef]) -> Self {
        let mut merged: HashMap<String, MethodMap> = HashMap::new();
        for r in routes {
            merged
                .entry(r.normalized_path())
                .or_default()
                .insert(r.method.clone(), Arc::clone(&r.handler));
        }

        let mut inner = matchit::Router::new();
        let mut listing = Vec::new();
        let mut paths: Vec<String> = merged.keys().cloned().collect();
        paths.sort();
        for path in paths {
            let map = merged.remove(&path).unwrap_or_default();
            for method in map.keys() {
                listing.push((method.clone(), path.clone()));
            }
            if let Err(err) = inner.insert(to_route_syntax(&path), map) {
                // Ruta duplicada: la macro lo impide (RDD0304); aquí queda registrado.
                tracing::error!(path, error = %err, "ruta duplicada o inválida al montar");
            }
        }
        listing.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.as_str().cmp(b.0.as_str())));
        Self { inner, listing }
    }

    /// Mapa de métodos para una ruta, con los parámetros capturados.
    fn at(&self, path: &str) -> Option<(&MethodMap, Vec<(String, String)>)> {
        let found = self.inner.at(path).ok()?;
        let params = found
            .params
            .iter()
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .collect();
        Some((found.value, params))
    }

    fn routes(&self) -> Vec<(Method, String)> {
        self.listing.clone()
    }
}

/// `/:id` → `/{id}`, la sintaxis que entiende `matchit` (RP-003).
/// `/:nombre` → `{nombre}`: la sintaxis que entienden `matchit` y Axum (RP-003,
/// resuelve V1). La comparten `dispatch` y las plataformas; el usuario siempre
/// declara `/:nombre`.
pub fn to_route_syntax(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    let mut chars = path.chars().peekable();
    while let Some(c) = chars.next() {
        if c == ':' {
            out.push('{');
            while let Some(&n) = chars.peek() {
                if n == '/' || n == '{' || n == '}' {
                    break;
                }
                out.push(n);
                chars.next();
            }
            out.push('}');
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;

    fn ok(body: &'static str) -> CoreHandler {
        Arc::new(move |_req: Request| {
            let body = Bytes::from_static(body.as_bytes());
            Box::pin(async move { Ok(Response::new(body)) })
        })
    }

    fn failing() -> CoreHandler {
        Arc::new(|_req| Box::pin(async move { Err(Error::not_found("no está")) }))
    }

    fn app(routes: Vec<(Method, &'static str, CoreHandler)>) -> HttpApp {
        app_with(routes, Vec::new())
    }

    fn app_with(
        routes: Vec<(Method, &'static str, CoreHandler)>,
        globals: Vec<BoxedMiddleware>,
    ) -> HttpApp {
        let routes = routes
            .into_iter()
            .map(|(method, path, chain)| RouteDef::from_chain(method, path, chain))
            .collect();
        HttpApp::assemble(
            routes,
            globals,
            Arc::new(Container::empty()),
            Limits::default(),
        )
    }

    fn req(method: Method, uri: &str) -> Request {
        http::Request::builder()
            .method(method)
            .uri(uri)
            .body(Bytes::new())
            .unwrap()
    }

    #[test]
    fn traduce_los_parametros_de_ruta() {
        assert_eq!(to_route_syntax("/tasks/:id"), "/tasks/{id}");
        assert_eq!(
            to_route_syntax("/tasks/:id/complete"),
            "/tasks/{id}/complete"
        );
        assert_eq!(to_route_syntax("/tasks"), "/tasks");
    }

    #[test]
    fn normaliza_la_barra_final() {
        assert_eq!(normalize_path("/tasks/"), "/tasks");
        assert_eq!(normalize_path("/tasks"), "/tasks");
        assert_eq!(normalize_path("/"), "/");
    }

    #[tokio::test]
    async fn resuelve_una_ruta_con_parametro() {
        let app = app(vec![(Method::GET, "/tasks/:id", ok("ok"))]);
        let resp = app.dispatch(req(Method::GET, "/tasks/abc")).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn la_barra_final_no_cambia_la_ruta() {
        let app = app(vec![(Method::GET, "/tasks", ok("ok"))]);
        for uri in ["/tasks", "/tasks/"] {
            let resp = app.dispatch(req(Method::GET, uri)).await;
            assert_eq!(resp.status(), StatusCode::OK, "en {uri}");
        }
    }

    #[tokio::test]
    async fn app_vacia_responde_404_a_todo() {
        let app = HttpApp::empty(Arc::new(Container::empty()), Limits::default());
        let resp = app.dispatch(req(Method::GET, "/")).await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn ruta_inexistente_devuelve_404_en_problem_details() {
        let app = app(Vec::new());
        let resp = app.dispatch(req(Method::GET, "/nada")).await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            resp.headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok()),
            Some("application/problem+json")
        );
    }

    #[tokio::test]
    async fn metodo_no_permitido_devuelve_405_con_allow() {
        let app = app(vec![
            (Method::GET, "/tasks", ok("ok")),
            (Method::DELETE, "/tasks", ok("ok")),
        ]);
        let resp = app.dispatch(req(Method::POST, "/tasks")).await;
        assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
        assert_eq!(
            resp.headers().get("allow").and_then(|v| v.to_str().ok()),
            Some("DELETE, GET")
        );
    }

    #[tokio::test]
    async fn cuerpo_fuera_de_limite_devuelve_413_sin_invocar_el_handler() {
        let mut app = app(vec![(Method::POST, "/tasks", ok("ok"))]);
        app.limits.body_limit_bytes = 4;
        let mut request = req(Method::POST, "/tasks");
        *request.body_mut() = Bytes::from_static(b"mucho texto");
        let resp = app.dispatch(request).await;
        assert_eq!(resp.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[tokio::test]
    async fn un_panic_en_el_handler_devuelve_500_y_no_rompe_nada() {
        let explosivo: CoreHandler = Arc::new(|_req| Box::pin(async move { panic!("explota") }));
        let app = app(vec![
            (Method::GET, "/boom", explosivo),
            (Method::GET, "/tasks", ok("ok")),
        ]);
        let resp = app.dispatch(req(Method::GET, "/boom")).await;
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let resp = app.dispatch(req(Method::GET, "/tasks")).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn un_error_del_handler_se_materializa_en_problem_details() {
        let app = app(vec![(Method::GET, "/tasks", failing())]);
        let resp = app.dispatch(req(Method::GET, "/tasks")).await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            resp.headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok()),
            Some("application/problem+json")
        );
    }

    #[tokio::test]
    async fn el_contenedor_esta_disponible_para_los_globales() {
        use crate::http::types::fn_middleware;
        use std::sync::atomic::{AtomicBool, Ordering};

        let visto = Arc::new(AtomicBool::new(false));
        let probe = {
            let visto = Arc::clone(&visto);
            fn_middleware(move |req: Request, next| {
                let visto = Arc::clone(&visto);
                async move {
                    visto.store(
                        req.extensions().get::<Arc<Container>>().is_some(),
                        Ordering::SeqCst,
                    );
                    next.run(req).await
                }
            })
        };
        let app = app_with(
            vec![(Method::GET, "/tasks", ok("ok"))],
            vec![Arc::new(probe)],
        );
        let resp = app.dispatch(req(Method::GET, "/tasks")).await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(
            visto.load(Ordering::SeqCst),
            "el global no vio el contenedor"
        );
    }

    #[tokio::test]
    async fn inventario_de_rutas_ordenado() {
        let app = app(vec![
            (Method::POST, "/tasks", ok("ok")),
            (Method::GET, "/tasks/:id", ok("ok")),
            (Method::GET, "/tasks", ok("ok")),
        ]);
        assert_eq!(
            app.route_listing(),
            vec!["GET /tasks", "POST /tasks", "GET /tasks/:id"]
        );
    }
}
