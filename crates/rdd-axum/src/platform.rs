//! `AxumPlatform`: sirve el `HttpApp` del núcleo sobre Axum (§4.4, RP-003).
//!
//! Registra las rutas de forma **nativa** en el router de Axum —no pasa por
//! `HttpApp::dispatch`— para aprovechar su enrutado y el soporte de capas Tower
//! (RP-011). Todo lo semántico (middlewares, errores, 404/405/413, pánicos) lo
//! hace el núcleo; aquí solo se traduce entre los dos mundos.

use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;

use axum::extract::{Path, Request};
use axum::response::Response;
use axum::routing::{Route, any};
use axum::{Router, routing::MethodRouter};
use rdd_core::error::{PlatformError, PlatformResult};
use rdd_core::http::{
    BoxHandler, HttpApp, Method, Platform, PlatformFuture, RouteDef, ShutdownSignal,
    normalize_path, to_route_syntax,
};
use tokio::net::TcpListener;
use tower::util::BoxCloneSyncService;
use tower::{Layer, Service};

use crate::shim::{RouteEntry, serve_fallback, serve_route};

/// El servicio ya borrado que produce una capa Tower sobre una ruta de `rdd`.
///
/// `BoxCloneSyncService` es lo que Axum usa internamente (`axum::routing::Route`
/// es un alias suyo), así que borrar aquí no añade un segundo nivel de caja.
type ErasedService = BoxCloneSyncService<Request, Response, Infallible>;

/// Capa Tower con el tipo concreto borrado.
///
/// `AxumPlatform` guarda un `Vec` de capas, y un `Vec` exige un único tipo; por
/// eso se borra el servicio en `ErasedService`, que es exactamente lo que
/// `MethodRouter::layer` admite.
#[derive(Clone)]
struct ErasedLayer(Arc<dyn Fn(Route) -> ErasedService + Send + Sync>);

impl Layer<Route> for ErasedLayer {
    type Service = ErasedService;

    fn layer(&self, inner: Route) -> Self::Service {
        (self.0)(inner)
    }
}

/// Plataforma por defecto de `rdd` (RP-012).
#[derive(Clone)]
pub struct AxumPlatform {
    layers: Vec<ErasedLayer>,
    #[cfg(feature = "graphql")]
    graphql: Vec<Arc<rdd_graphql::GraphqlHttp>>,
    #[cfg(feature = "mcp-http")]
    mcp: Vec<Arc<rdd_mcp::McpHttp>>,
}

impl Default for AxumPlatform {
    fn default() -> Self {
        Self::new()
    }
}

impl AxumPlatform {
    /// Plataforma sin capas Tower adicionales.
    pub fn new() -> Self {
        Self {
            layers: Vec::new(),
            #[cfg(feature = "graphql")]
            graphql: Vec::new(),
            #[cfg(feature = "mcp-http")]
            mcp: Vec::new(),
        }
    }

    /// Monta un `GraphqlHttp` en su ruta (RF-233, RP-030).
    ///
    /// La ruta se sirve con `any(...)` para que la cadena global (CORS, JWT,
    /// RateLimit) decida antes que el protocolo. Un choque con una ruta HTTP, un
    /// canal, `/mcp` u otro `/graphql`, o una configuración inválida, impide
    /// arrancar.
    #[cfg(feature = "graphql")]
    pub fn graphql(mut self, graphql: rdd_graphql::GraphqlHttp) -> Self {
        self.graphql.push(Arc::new(graphql));
        self
    }

    /// Monta un `McpHttp` en su ruta (RF-222, RP-029).
    ///
    /// La ruta se sirve con `any(...)` para que la cadena global (CORS, JWT,
    /// RateLimit) decida antes que el protocolo. Un choque con una ruta HTTP o
    /// un canal, o una ruta inválida o repetida, impide arrancar.
    #[cfg(feature = "mcp-http")]
    pub fn mcp(mut self, mcp: rdd_mcp::McpHttp) -> Self {
        self.mcp.push(Arc::new(mcp));
        self
    }

    /// Añade una capa Tower al servidor (RP-011).
    ///
    /// Las capacidades del servidor se configuran aquí, nunca en `App`. La capa
    /// queda **por fuera** de la cadena de `rdd` (RP-005): no se inserta entre
    /// sus middlewares ni altera el orden de §11.3.
    pub fn layer<L>(mut self, layer: L) -> Self
    where
        L: Layer<Route> + Clone + Send + Sync + 'static,
        L::Service: Service<Request, Response = Response, Error = Infallible>
            + Clone
            + Send
            + Sync
            + 'static,
        <L::Service as Service<Request>>::Future: Send + 'static,
    {
        self.layers.push(ErasedLayer(Arc::new(move |inner| {
            BoxCloneSyncService::new(layer.clone().layer(inner))
        })));
        self
    }

    /// Construye el router de Axum con las rutas del núcleo (RP-003, RP-008).
    fn router(&self, app: Arc<HttpApp>, shutdown: ShutdownSignal) -> PlatformResult<Router> {
        #[cfg(feature = "mcp-http")]
        validate_mcp(&self.mcp, &app)?;
        #[cfg(feature = "graphql")]
        {
            #[cfg(feature = "mcp-http")]
            let mcp_paths: Vec<String> = self
                .mcp
                .iter()
                .map(|mcp| normalize_path(mcp.path()))
                .collect();
            #[cfg(not(feature = "mcp-http"))]
            let mcp_paths: Vec<String> = Vec::new();
            validate_graphql(&self.graphql, &mcp_paths, &app)?;
        }
        let limits = app.limits;
        #[cfg(not(feature = "ws"))]
        let _ = shutdown;
        let fallback = Arc::clone(&app.fallback);
        let fallback_service = Arc::clone(&fallback);
        let fallback_handler = move |req: Request| {
            let fallback = Arc::clone(&fallback_service);
            async move { serve_fallback(fallback, limits, req).await }
        };

        let mut router = Router::new().fallback(fallback_handler);
        for (declared, methods) in group_by_path(&app.routes) {
            let entry = Arc::new(RouteEntry::new(methods, Arc::clone(&fallback), limits));
            let handler_entry = Arc::clone(&entry);
            let handler = move |params: Path<HashMap<String, String>>, req: Request| {
                let entry = Arc::clone(&handler_entry);
                async move { serve_route(entry, params.0, req).await }
            };

            let pattern = to_route_syntax(&declared);
            // RP-008: Axum no fusiona `/tasks/` con `/tasks`; se registran las
            // dos grafías y el shim normaliza antes de entregar al núcleo.
            for spelling in spellings(&pattern) {
                router = router.route(&spelling, apply_layers(any(handler.clone()), &self.layers));
            }
        }

        #[cfg(feature = "mcp-http")]
        for mcp in &self.mcp {
            let path = normalize_path(mcp.path());
            let handler = {
                let app = Arc::clone(&app);
                let mcp = Arc::clone(mcp);
                move |req: Request| {
                    let app = Arc::clone(&app);
                    let mcp = Arc::clone(&mcp);
                    async move { crate::mcp_http::serve_mcp(app, mcp, req).await }
                }
            };
            for spelling in spellings(&path) {
                router = router.route(&spelling, apply_layers(any(handler.clone()), &self.layers));
            }
        }

        #[cfg(feature = "graphql")]
        for graphql in &self.graphql {
            let path = normalize_path(graphql.path());
            let handler = {
                let app = Arc::clone(&app);
                let graphql = Arc::clone(graphql);
                move |req: Request| {
                    let app = Arc::clone(&app);
                    let graphql = Arc::clone(&graphql);
                    async move { crate::graphql_http::serve_graphql(app, graphql, req).await }
                }
            };
            for spelling in spellings(&path) {
                router = router.route(&spelling, apply_layers(any(handler.clone()), &self.layers));
            }
        }

        for upgrade in &app.upgrades {
            let pattern = to_route_syntax(&upgrade.path);
            #[cfg(feature = "ws")]
            let handler = {
                let app = Arc::clone(&app);
                let shutdown = shutdown.clone();
                move |params: Path<HashMap<String, String>>, req: Request| {
                    let app = Arc::clone(&app);
                    let shutdown = shutdown.clone();
                    async move { crate::ws::serve_upgrade(app, shutdown, params.0, req).await }
                }
            };
            #[cfg(not(feature = "ws"))]
            let handler = {
                let app = Arc::clone(&app);
                move |req: Request| {
                    let app = Arc::clone(&app);
                    async move {
                        let resp = app.upgrade_not_implemented(req.uri().path());
                        core_response(resp)
                    }
                }
            };

            for spelling in spellings(&pattern) {
                router = router.route(&spelling, apply_layers(any(handler.clone()), &self.layers));
            }
        }
        Ok(router)
    }
}

impl Platform for AxumPlatform {
    fn name(&self) -> &'static str {
        "axum"
    }

    fn supports_upgrades(&self) -> bool {
        cfg!(feature = "ws")
    }

    fn serve(
        self: Arc<Self>,
        app: HttpApp,
        listener: TcpListener,
        shutdown: ShutdownSignal,
    ) -> PlatformResult<PlatformFuture<Result<(), PlatformError>>> {
        let grace = app.limits.shutdown_grace;
        let app = Arc::new(app);
        let router = self.router(Arc::clone(&app), shutdown.clone())?;

        // RF-229: al apagar, `/mcp` cancela lo que esté en vuelo y cierra los
        // flujos SSE para que el drenaje de Axum pueda terminar.
        #[cfg(feature = "mcp-http")]
        for mcp in &self.mcp {
            let mcp = Arc::clone(mcp);
            let shutdown = shutdown.clone();
            tokio::spawn(async move {
                shutdown.wait().await;
                mcp.shutdown().await;
            });
        }

        // RF-238: al apagar, `/graphql` cancela las suscripciones en vuelo para
        // que el drenaje de Axum pueda terminar.
        #[cfg(feature = "graphql")]
        for graphql in &self.graphql {
            let graphql = Arc::clone(graphql);
            let shutdown = shutdown.clone();
            tokio::spawn(async move {
                shutdown.wait().await;
                graphql.shutdown().await;
            });
        }

        // RP-009: al activarse la señal se dejan de aceptar conexiones nuevas y
        // se drenan las en curso. El plazo `shutdown_grace` se impone por fuera:
        // si vence, se aborta la tarea del servidor.
        let shutdown_for_server = shutdown.clone();
        let server = async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(async move { shutdown_for_server.wait().await })
                .await
        };
        let running = tokio::spawn(server);
        let deadline = async move {
            shutdown.wait().await;
            tokio::time::sleep(grace).await;
        };

        Ok(Box::pin(async move {
            tokio::select! {
                joined = running => match joined {
                    Ok(Ok(())) => Ok(()),
                    Ok(Err(err)) => Err(PlatformError::Server {
                        platform: "axum",
                        message: err.to_string(),
                    }),
                    Err(err) => Err(PlatformError::Server {
                        platform: "axum",
                        message: err.to_string(),
                    }),
                },
                _ = deadline => {
                    tracing::warn!(
                        grace_ms = grace.as_millis(),
                        "el drenaje de axum superó el plazo de apagado: se cierra el servidor"
                    );
                    Ok(())
                }
            }
        }))
    }
}

/// Aplica las capas Tower por fuera de la cadena de `rdd` (RP-005).
fn apply_layers(mut router: MethodRouter, layers: &[ErasedLayer]) -> MethodRouter {
    for layer in layers {
        router = router.layer(layer.clone());
    }
    router
}

/// Respuesta del núcleo al tipo de Axum, sin transportar canales (RP-027).
#[cfg(not(feature = "ws"))]
fn core_response(resp: rdd_core::http::Response) -> Response {
    let (parts, body) = resp.into_parts();
    Response::from_parts(parts, axum::body::Body::from(body))
}

/// Rutas del núcleo agrupadas por ruta declarada, conservando el orden de montaje.
fn group_by_path(routes: &[RouteDef]) -> Vec<(String, HashMap<Method, BoxHandler>)> {
    let mut grouped: Vec<(String, HashMap<Method, BoxHandler>)> = Vec::new();
    for def in routes {
        if let Some((_, methods)) = grouped.iter_mut().find(|(path, _)| path == &def.path) {
            methods.insert(def.method.clone(), Arc::clone(&def.handler));
            continue;
        }
        let mut methods = HashMap::new();
        methods.insert(def.method.clone(), Arc::clone(&def.handler));
        grouped.push((def.path.clone(), methods));
    }
    grouped
}

/// Las dos grafías de una ruta, para cubrir la barra final (RP-008).
fn spellings(pattern: &str) -> Vec<String> {
    if pattern == "/" || pattern.ends_with('/') {
        return vec![pattern.to_owned()];
    }
    vec![pattern.to_owned(), format!("{pattern}/")]
}

/// Comprueba el montaje GraphQL antes de arrancar (RF-233, RF-242).
#[cfg(feature = "graphql")]
fn validate_graphql(
    graphqls: &[Arc<rdd_graphql::GraphqlHttp>],
    mcp_paths: &[String],
    app: &HttpApp,
) -> PlatformResult<()> {
    let mut seen: Vec<String> = Vec::new();
    for graphql in graphqls {
        let settings = graphql.settings();
        settings.validate().map_err(|err| {
            PlatformError::other(anyhow::anyhow!("configuración GraphQL inválida: {err}"))
        })?;

        let raw = settings.mount_path();
        let path = normalize_path(raw);
        let literal = raw.starts_with('/')
            && raw != "/"
            && raw == path
            && !raw.contains([':', '*', '{', '}', '?', ' ', '\t', '\n']);
        if !literal {
            return Err(PlatformError::other(anyhow::anyhow!(
                "ruta GraphQL inválida `{raw}`: se espera una ruta literal como `/graphql`"
            )));
        }
        if seen.contains(&path) {
            return Err(PlatformError::other(anyhow::anyhow!(
                "ruta GraphQL duplicada `{path}`: ya hay un montaje en esa ruta"
            )));
        }
        if app
            .routes
            .iter()
            .any(|def| normalize_path(&def.path) == path)
        {
            return Err(PlatformError::other(anyhow::anyhow!(
                "la ruta GraphQL `{path}` colisiona con una ruta HTTP"
            )));
        }
        if app
            .upgrades
            .iter()
            .any(|upgrade| normalize_path(&upgrade.path) == path)
        {
            return Err(PlatformError::other(anyhow::anyhow!(
                "la ruta GraphQL `{path}` colisiona con un canal de actualización"
            )));
        }
        if mcp_paths.contains(&path) {
            return Err(PlatformError::other(anyhow::anyhow!(
                "la ruta GraphQL `{path}` colisiona con el montaje `/mcp`"
            )));
        }
        seen.push(path);
    }
    Ok(())
}

/// Comprueba rutas de montaje antes de arrancar (RF-222).
#[cfg(feature = "mcp-http")]
fn validate_mcp(mcps: &[Arc<rdd_mcp::McpHttp>], app: &HttpApp) -> PlatformResult<()> {
    let mut seen: Vec<String> = Vec::new();
    for mcp in mcps {
        let raw = mcp.path();
        let path = normalize_path(raw);
        let literal = raw.starts_with('/')
            && raw != "/"
            && raw == path
            && !raw.contains([':', '*', '{', '}', '?', ' ', '\t', '\n']);
        if !literal {
            return Err(PlatformError::other(anyhow::anyhow!(
                "ruta MCP inválida `{raw}`: se espera una ruta literal como `/mcp`"
            )));
        }
        if seen.contains(&path) {
            return Err(PlatformError::other(anyhow::anyhow!(
                "ruta MCP duplicada `{path}`: ya hay un montaje en esa ruta"
            )));
        }
        if app
            .routes
            .iter()
            .any(|def| normalize_path(&def.path) == path)
        {
            return Err(PlatformError::other(anyhow::anyhow!(
                "la ruta MCP `{path}` colisiona con una ruta HTTP"
            )));
        }
        if app
            .upgrades
            .iter()
            .any(|upgrade| normalize_path(&upgrade.path) == path)
        {
            return Err(PlatformError::other(anyhow::anyhow!(
                "la ruta MCP `{path}` colisiona con un canal de actualización"
            )));
        }
        seen.push(path);
    }
    Ok(())
}
