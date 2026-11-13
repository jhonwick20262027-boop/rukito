//! Extractores del núcleo (RF-054, RF-057). Implementan [`FromRequest`] sobre un
//! [`Ctx`] propietario, sin depender de ninguna plataforma.

use std::future::Future;
use std::ops::Deref;
use std::str::FromStr;
use std::sync::Arc;

pub use bytes::Bytes;

use crate::di::Container;
use crate::error::{Error, Problem};
use crate::http::response::problem_response;
use crate::http::types::{Request, Response, StatusCode, header};

/// Parámetros de ruta capturados, inyectados por la plataforma (RP-003).
///
/// La sintaxis declarada por el usuario es siempre `/:nombre` (§10.2); cada
/// plataforma traduce a la de su router y rellena esta lista.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PathParams {
    pairs: Vec<(String, String)>,
}

impl PathParams {
    /// Crea la lista a partir de pares `(nombre, valor)` ya decodificados.
    pub fn new(pairs: Vec<(String, String)>) -> Self {
        Self { pairs }
    }

    /// Valor crudo del parámetro `nombre`.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.pairs
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    /// Iterador sobre los pares capturados, en el orden de la ruta.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.pairs.iter().map(|(n, v)| (n.as_str(), v.as_str()))
    }

    /// Número de parámetros.
    pub fn len(&self) -> usize {
        self.pairs.len()
    }

    /// Cierto si no hay parámetros.
    pub fn is_empty(&self) -> bool {
        self.pairs.is_empty()
    }

    /// Convierte el parámetro `name` en `T`, con 400 legible si falla (C-04).
    pub fn parse<T: FromStr>(&self, name: &str) -> Result<T, Error> {
        let raw = self.get(name).ok_or_else(|| {
            let disponibles: Vec<&str> = self.iter().map(|(n, _)| n).collect();
            Error::bad_request(format!(
                "la ruta no declara el parámetro `{name}` (disponibles: {disponibles:?})"
            ))
        })?;
        T::from_str(raw).map_err(|_| {
            Error::bad_request(format!(
                "el parámetro `{name}` recibe `{raw}`, que no es un valor válido"
            ))
        })
    }
}

/// Pista de enlace que la macro pasa al extractor: de qué parámetro de ruta o de
/// qué cabecera procede el valor.
///
/// `Json` y `Query` lo ignoran; `Path` y `Header` lo necesitan, y la firma única
/// del trait (RF-057) los obliga a recibirlo.
#[derive(Debug, Clone, Copy, Default)]
pub struct Bind {
    /// Nombre del parámetro de ruta, sin `:`.
    pub param: Option<&'static str>,
    /// Nombre de la cabecera solicitada.
    pub header: Option<&'static str>,
}

impl Bind {
    /// Sin pista de enlace.
    pub fn none() -> Self {
        Self::default()
    }

    /// Enlace a un parámetro de ruta.
    pub const fn param(name: &'static str) -> Self {
        Self {
            param: Some(name),
            header: None,
        }
    }

    /// Enlace a una cabecera.
    pub const fn header(name: &'static str) -> Self {
        Self {
            param: None,
            header: Some(name),
        }
    }
}

/// Contexto de petición que recibe cada extractor (RF-057).
///
/// Es un `Request` con vida propia: los extractores devuelven datos de propiedad
/// (`Bytes` clonado, `String`, `Arc`), de modo que el futuro de `async fn` solo
/// atrapa el borrow del `Ctx` local y se puede encadenar sin límite de aridad.
pub struct Ctx {
    req: Request,
}

impl Ctx {
    /// Envuelve una petición ya leída.
    pub fn of(req: Request) -> Self {
        Self { req }
    }

    /// Petición subyacente.
    pub fn request(&self) -> &Request {
        &self.req
    }

    /// Ruta solicitada.
    pub fn path(&self) -> &str {
        self.req.uri().path()
    }

    /// Cabecera como texto UTF-8.
    pub fn header_str(&self, name: header::HeaderName) -> Option<&str> {
        self.req.headers().get(name).and_then(|v| v.to_str().ok())
    }

    /// Query string cruda, sin el `?`.
    pub fn raw_query(&self) -> &str {
        self.req.uri().query().unwrap_or("")
    }

    /// Parámetros de ruta puestos por la plataforma o por `dispatch` (RP-003).
    pub fn params(&self) -> PathParams {
        self.req
            .extensions()
            .get::<PathParams>()
            .cloned()
            .unwrap_or_default()
    }

    /// Métodos permitidos en la ruta, si la plataforma los informó (RP-007).
    pub fn allowed_methods(&self) -> Option<crate::http::response::AllowedMethods> {
        self.req
            .extensions()
            .get::<crate::http::response::AllowedMethods>()
            .cloned()
    }

    /// Contenedor de dependencias, inyectado por el núcleo en las extensiones.
    pub fn container(&self) -> Arc<Container> {
        if let Some(c) = self.req.extensions().get::<Arc<Container>>() {
            return Arc::clone(c);
        }
        Arc::clone(&EMPTY_CONTAINER)
    }

    /// Valor colocado en las extensiones por un middleware (RF-083).
    pub fn ext<T: Clone + Send + Sync + 'static>(&self) -> Result<T, Error> {
        self.req.extensions().get::<T>().cloned().ok_or_else(|| {
            Error::bad_request(format!(
                "ningún middleware puso `{}` en las extensiones (RF-083)",
                std::any::type_name::<T>()
            ))
        })
    }

    /// Servicio del contenedor (RF-022).
    pub fn resolve<T>(&self) -> Result<T, Error>
    where
        T: FromContainer,
    {
        T::from_container(&self.container())
    }

    /// Cuerpo de la petición, ya en memoria (RP-002).
    pub fn body(&self) -> Bytes {
        self.req.body().clone()
    }

    /// Desmonta el contexto y devuelve la petición, con sus extensiones intactas.
    pub fn into_request(self) -> Request {
        self.req
    }
}

static EMPTY_CONTAINER: std::sync::LazyLock<Arc<Container>> =
    std::sync::LazyLock::new(|| Arc::new(Container::empty()));

/// Valor construible desde el contenedor (RF-022).
///
/// Vive en [`crate::di`] porque §6.3 lo expone como `rdd::di::FromContainer`; se
/// re-exporta aquí para que `rdd::http::FromContainer` también resuelva.
pub use crate::di::FromContainer;

/// Contracto de extractor (RF-057).
pub trait FromRequest: Send + Sized {
    /// Extrae `Self` del contexto. Un fallo DEBE producir 400 con Problem Details
    /// (RF-055); un fallo de infraestructura, 500.
    fn from_request(ctx: &Ctx, bind: Bind) -> impl Future<Output = Result<Self, Error>> + Send;

    /// Enlace declarado por el propio extractor, usado por la macro cuando el
    /// parámetro de la firma no lleva nombre explícito.
    const PARAM: Option<&'static str> = None;
}

/// Nombre del tipo para los mensajes de error de dependencia faltante.
pub fn type_name<T: ?Sized>() -> &'static str {
    std::any::type_name::<T>()
}

// ---------------------------------------------------------------------------
// Json<T>
// ---------------------------------------------------------------------------

/// Cuerpo JSON validado al extraer (RF-054). `Deref` hacia su contenido.
#[derive(Debug, Clone)]
pub struct Json<T>(T);

impl<T> Json<T> {
    /// Desmonta el envoltorio.
    pub fn into_inner(self) -> T {
        self.0
    }

    /// Referencia al valor.
    pub fn inner(&self) -> &T {
        &self.0
    }
}

impl<T> Deref for Json<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> FromRequest for Json<T>
where
    T: serde::de::DeserializeOwned + Send,
{
    async fn from_request(ctx: &Ctx, _bind: Bind) -> Result<Self, Error> {
        let body = ctx.body();
        if body.is_empty() {
            return Err(Error::bad_request("el cuerpo JSON está vacío"));
        }
        let ct = ctx.header_str(header::CONTENT_TYPE).unwrap_or("");
        if !ct.is_empty() && !ct.contains("application/json") {
            return Err(Error::bad_request(format!(
                "content-type `{ct}` no soportado: use application/json"
            )));
        }
        serde_json::from_slice::<T>(&body)
            .map(Json)
            .map_err(|err| Error::bad_request(format!("JSON inválido: {err}")))
    }
}

// ---------------------------------------------------------------------------
// Path<T>
// ---------------------------------------------------------------------------

/// Parámetro de ruta convertido con `FromStr` (RF-054, C-04).
#[derive(Debug, Clone)]
pub struct Path<T>(T);

impl<T> Path<T> {
    /// Envuelve un valor ya convertido.
    pub fn new(value: T) -> Self {
        Self(value)
    }

    /// Desmonta el envoltorio.
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T> Deref for Path<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> From<T> for Path<T> {
    fn from(value: T) -> Self {
        Self(value)
    }
}

impl<T> FromRequest for Path<T>
where
    T: FromStr + Send,
    T::Err: std::fmt::Debug,
{
    async fn from_request(ctx: &Ctx, bind: Bind) -> Result<Self, Error> {
        let params = ctx.params();
        // Con nombre enlazado (caso normal, la macro lo pasa) o por posición.
        if let Some(name) = bind.param {
            return params.parse::<T>(name).map(Path);
        }
        let (name, raw) = params
            .iter()
            .next()
            .ok_or_else(|| Error::bad_request("la ruta no declara parámetros"))?;
        T::from_str(raw).map(Path).map_err(|_| {
            Error::bad_request(format!(
                "el parámetro `{name}` recibe `{raw}`, que no es un valor válido"
            ))
        })
    }
}

// ---------------------------------------------------------------------------
// Query<T>
// ---------------------------------------------------------------------------

/// Parámetros de query deserializados con serde (RF-054).
#[derive(Debug, Clone)]
pub struct Query<T>(T);

impl<T> Query<T> {
    /// Desmonta el envoltorio.
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T> Deref for Query<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> FromRequest for Query<T>
where
    T: serde::de::DeserializeOwned + Send,
{
    async fn from_request(ctx: &Ctx, _bind: Bind) -> Result<Self, Error> {
        let pairs = crate::http::query::parse_query(ctx.raw_query());
        T::deserialize(crate::http::query::QueryPairs::new(&pairs))
            .map(Query)
            .map_err(|err| Error::bad_request(format!("query inválida: {err}")))
    }
}

// ---------------------------------------------------------------------------
// Header<T>
// ---------------------------------------------------------------------------

/// Cabecera concreta, convertida con `FromStr` (RF-054).
#[derive(Debug)]
pub struct Header<T = String>(T);

impl<T> Header<T> {
    /// Desmonta el envoltorio.
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T> Deref for Header<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> FromRequest for Header<T>
where
    T: FromStr + Send,
    T::Err: std::fmt::Debug,
{
    async fn from_request(ctx: &Ctx, bind: Bind) -> Result<Self, Error> {
        let name = bind.header.unwrap_or("x-rdd-header");
        let raw = ctx
            .request()
            .headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| Error::bad_request(format!("falta la cabecera `{name}`")))?;
        T::from_str(raw)
            .map(Header)
            .map_err(|_| Error::bad_request(format!("cabecera `{name}` inválida: `{raw}`")))
    }
}

// ---------------------------------------------------------------------------
// Ext<T>
// ---------------------------------------------------------------------------

/// Valor puesto en las extensiones por un middleware (RF-083).
#[derive(Debug)]
pub struct Ext<T>(T);

impl<T> Ext<T> {
    /// Desmonta el envoltorio.
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T> Deref for Ext<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> FromRequest for Ext<T>
where
    T: Clone + Send + Sync + 'static,
{
    async fn from_request(ctx: &Ctx, _bind: Bind) -> Result<Self, Error> {
        ctx.ext::<T>().map(Ext)
    }
}

// ---------------------------------------------------------------------------
// Bytes
// ---------------------------------------------------------------------------

/// Cuerpo completo en memoria (RF-278).
///
/// El límite de cuerpo de la app (RF-017) ya se aplicó al leer la petición;
/// separar campos y archivos de un multiparte no es responsabilidad del núcleo.
impl FromRequest for Bytes {
    async fn from_request(ctx: &Ctx, _bind: Bind) -> Result<Self, Error> {
        Ok(ctx.body())
    }
}

// ---------------------------------------------------------------------------
// Problemas sueltos
// ---------------------------------------------------------------------------

/// Construye una respuesta Problem Details directamente (RF-082, §10.3).
pub fn problem(status: StatusCode, title: &str, detail: &str, instance: &str) -> Response {
    let problem = Problem {
        r#type: "about:blank".to_owned(),
        title: title.to_owned(),
        status: status.as_u16(),
        detail: detail.to_owned(),
        instance: instance.to_owned(),
    };
    problem_response(status, &problem, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::types::{Middleware, NextFn};
    use serde::Deserialize;

    fn ctx_with(uri: &str, params: &[(&str, &str)], body: &'static str) -> Ctx {
        let mut req = http::Request::builder()
            .method(http::Method::GET)
            .uri(uri)
            .body(Bytes::from_static(body.as_bytes()))
            .unwrap();
        if !params.is_empty() {
            req.extensions_mut().insert(PathParams::new(
                params
                    .iter()
                    .map(|(n, v)| ((*n).to_owned(), (*v).to_owned()))
                    .collect(),
            ));
        }
        Ctx::of(req)
    }

    async fn extract<T: FromRequest>(ctx: &Ctx, bind: Bind) -> crate::Result<T> {
        T::from_request(ctx, bind).await
    }

    #[test]
    fn enlaza_parametro_de_ruta_por_nombre() {
        let b = Bind::param("id");
        assert_eq!(b.param, Some("id"));
        assert_eq!(b.header, None);
    }

    #[tokio::test]
    async fn path_convierte_el_valor_declarado() {
        let ctx = ctx_with("/tasks/7", &[("id", "7")], "");
        let Path(n) = extract::<Path<u32>>(&ctx, Bind::param("id")).await.unwrap();
        assert_eq!(n, 7);
    }

    #[tokio::test]
    async fn path_invalido_devuelve_400_que_nombra_el_parametro() {
        // C-04: un UUID inválido en la ruta es 400, no 500.
        let ctx = ctx_with("/tasks/zzz", &[("id", "zzz")], "");
        let err = extract::<Path<uuid::Uuid>>(&ctx, Bind::param("id"))
            .await
            .unwrap_err();
        assert_eq!(err.status(), StatusCode::BAD_REQUEST);
        let detail = err.client_detail();
        assert!(detail.contains("id"), "{detail}");
        assert!(detail.contains("zzz"), "{detail}");
    }

    #[tokio::test]
    async fn path_sin_el_parametro_declarado_es_400() {
        let ctx = ctx_with("/tasks/1", &[("id", "1")], "");
        let err = extract::<Path<u32>>(&ctx, Bind::param("usuario"))
            .await
            .unwrap_err();
        assert_eq!(err.status(), StatusCode::BAD_REQUEST);
        assert!(err.client_detail().contains("usuario"));
    }

    #[derive(Debug, Deserialize, PartialEq)]
    struct CreateTask {
        title: String,
    }

    #[tokio::test]
    async fn json_valido_se_deserializa() {
        let ctx = ctx_with("/tasks", &[], r#"{"title":"comprar pan"}"#);
        let Json(value) = extract::<Json<CreateTask>>(&ctx, Bind::none())
            .await
            .unwrap();
        assert_eq!(
            value,
            CreateTask {
                title: "comprar pan".to_owned()
            }
        );
    }

    #[tokio::test]
    async fn json_invalido_es_400_y_cuerpo_vacio_tambien() {
        // C-06.
        let ctx = ctx_with("/tasks", &[], "{no es json");
        let err = extract::<Json<CreateTask>>(&ctx, Bind::none())
            .await
            .unwrap_err();
        assert_eq!(err.status(), StatusCode::BAD_REQUEST);

        let vacio = ctx_with("/tasks", &[], "");
        assert_eq!(
            extract::<Json<CreateTask>>(&vacio, Bind::none())
                .await
                .unwrap_err()
                .status(),
            StatusCode::BAD_REQUEST
        );
    }

    #[tokio::test]
    async fn content_type_incorrecto_es_400() {
        let mut req = http::Request::builder()
            .uri("/tasks")
            .header(header::CONTENT_TYPE, "text/plain")
            .body(Bytes::from_static(br#"{"title":"x"}"#))
            .unwrap();
        req.extensions_mut().insert(PathParams::default());
        let ctx = Ctx::of(req);
        let err = extract::<Json<CreateTask>>(&ctx, Bind::none())
            .await
            .unwrap_err();
        assert!(err.client_detail().contains("text/plain"));
    }

    #[tokio::test]
    async fn bytes_entrega_el_cuerpo_completo_y_admite_vacio() {
        // RF-278: el cuerpo en memoria se entrega tal cual, sin multiparte.
        let ctx = ctx_with("/files", &[], "contenido binario");
        let body = extract::<Bytes>(&ctx, Bind::none()).await.unwrap();
        assert_eq!(&body[..], b"contenido binario");

        let vacio = ctx_with("/files", &[], "");
        assert!(
            extract::<Bytes>(&vacio, Bind::none())
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[derive(Debug, Deserialize, PartialEq)]
    struct Filtro {
        q: String,
        page: Option<u32>,
    }
    #[tokio::test]
    async fn query_deserializa_y_decodifica_porcentaje() {
        let ctx = ctx_with("/tasks?q=pan%20quemado&page=2", &[], "");
        let Query(value) = extract::<Query<Filtro>>(&ctx, Bind::none()).await.unwrap();
        assert_eq!(
            value,
            Filtro {
                q: "pan quemado".to_owned(),
                page: Some(2)
            }
        );
    }

    #[tokio::test]
    async fn query_invalida_es_400() {
        let ctx = ctx_with("/tasks?page=nope", &[], "");
        let err = extract::<Query<Filtro>>(&ctx, Bind::none())
            .await
            .unwrap_err();
        assert_eq!(err.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn header_faltante_es_400() {
        let ctx = ctx_with("/tasks", &[], "");
        let err = extract::<Header<String>>(&ctx, Bind::header("x-token"))
            .await
            .unwrap_err();
        assert_eq!(err.status(), StatusCode::BAD_REQUEST);
        assert!(err.client_detail().contains("x-token"));
    }

    #[tokio::test]
    async fn extension_de_un_middleware_es_extraible() {
        #[derive(Debug, Clone)]
        struct Usuario(String);

        let ctx = ctx_with("/tasks", &[], "");
        // Sin middleware que la ponga → 400 con mensaje RF-083.
        let err = extract::<Ext<Usuario>>(&ctx, Bind::none())
            .await
            .unwrap_err();
        assert!(err.client_detail().contains("RF-083"));

        let mut req = http::Request::builder()
            .uri("/tasks")
            .body(Bytes::new())
            .unwrap();
        req.extensions_mut().insert(Usuario("ana".to_owned()));
        let ctx = Ctx::of(req);
        let Ext(usuario) = extract::<Ext<Usuario>>(&ctx, Bind::none()).await.unwrap();
        assert_eq!(usuario.0, "ana");
    }

    #[tokio::test]
    async fn un_middleware_de_prueba_puede_cortocircuitar() {
        // RF-082: el middleware devuelve Err sin llamar a next.
        use crate::http::types::NextArc;
        let mw = crate::http::types::fn_middleware(|_req, _next: NextFn| async move {
            Err(Error::Unauthorized)
        });
        let req = http::Request::builder()
            .uri("/tasks")
            .body(Bytes::new())
            .unwrap();
        let next: NextArc =
            Arc::new(|req| Box::pin(async move { Ok(Response::new(req.body().clone())) }));
        let err = mw.handle(req, next).await.unwrap_err();
        assert_eq!(err.status(), StatusCode::UNAUTHORIZED);
    }
}
