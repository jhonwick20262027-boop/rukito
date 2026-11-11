//! # rdd-cors
//!
//! CORS del framework (§11.5, ADR-13): implementación **propia** sobre el trait
//! [`Middleware`] del núcleo, sin Tower y sin ninguna plataforma, así que se
//! comporta igual sobre Axum que sobre hyper.
//!
//! Seguro por defecto: [`Cors::new`] sin configurar deniega todo origen cruzado
//! (RF-090). [`Cors::permissive`] abre todo y avisa en `release` (RF-091).
//! [`Cors::from_config`] lee la sección `[cors]` del `rdd.toml` del directorio
//! actual (RF-093).
//!
//! ```no_run
//! use std::time::Duration;
//!
//! use rdd_core::http::Method;
//! use rdd_core::prelude::*;
//! use rdd_cors::Cors;
//!
//! # fn montar() -> Result<(), rdd_cors::CorsConfigError> {
//! let app = App::new().middleware(
//!     Cors::new()
//!         .allow_origin("https://miapp.com")
//!         .allow_methods([Method::GET, Method::POST])
//!         .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION])
//!         .expose_headers(["x-request-id", "ratelimit-remaining"])
//!         .allow_credentials(true)
//!         .max_age(Duration::from_secs(3600))
//!         .build()?,
//! );
//! # let _ = app;
//! # Ok(())
//! # }
//! ```

mod config;

use std::fmt::Display;
use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::str::FromStr;
use std::time::Duration;

use bytes::Bytes;
use http::header::{
    ACCESS_CONTROL_ALLOW_CREDENTIALS, ACCESS_CONTROL_ALLOW_HEADERS, ACCESS_CONTROL_ALLOW_METHODS,
    ACCESS_CONTROL_ALLOW_ORIGIN, ACCESS_CONTROL_EXPOSE_HEADERS, ACCESS_CONTROL_MAX_AGE,
    ACCESS_CONTROL_REQUEST_HEADERS, ACCESS_CONTROL_REQUEST_METHOD, ORIGIN, VARY,
};
use http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode};
use rdd_core::http::{Middleware, NextArc, Request, Response};

/// Métodos safelisted por el estándar Fetch: lo que anuncia un preflight cuando
/// no se configuró `allow_methods` (§11.5).
const SAFELISTED_METHODS: [Method; 3] = [Method::GET, Method::HEAD, Method::POST];

/// Cabeceras safelisted por el estándar Fetch, para `allow_headers` sin configurar.
const SAFELISTED_HEADERS: [&str; 4] = [
    "accept",
    "accept-language",
    "content-language",
    "content-type",
];

/// Middleware CORS ya validado. Es lo único que acepta `App::middleware`.
///
/// Se construye con [`Cors::new`] + [`CorsBuilder::build`], con
/// [`Cors::permissive`] o con [`Cors::from_config`].
#[derive(Debug, Clone)]
pub struct Cors {
    origins: Vec<String>,
    wildcard: bool,
    /// `permissive()`: el preflight refleja el método y las cabeceras que pida.
    reflect: bool,
    allow_methods: HeaderValue,
    allow_headers: HeaderValue,
    expose_headers: Option<HeaderValue>,
    credentials: bool,
    max_age: Option<HeaderValue>,
}

impl Cors {
    /// Builder vacío: deniega todo origen cruzado hasta que se configure (RF-090).
    // §11.5 fija esta forma: `new()` devuelve el builder y `build()` el middleware
    // validado, para poder encadenar setters sin un `?` por línea (RF-092).
    #[allow(clippy::new_ret_no_self)]
    pub fn new() -> CorsBuilder {
        CorsBuilder::default()
    }

    /// Cualquier origen, método y cabecera, sin credenciales. Pensado para
    /// desarrollo: en perfil `release` emite un `tracing::warn!` (RF-091).
    ///
    /// No admite credenciales a propósito: `*` con `Allow-Credentials` es la
    /// combinación que el estándar prohíbe (RF-092).
    pub fn permissive() -> Self {
        if !cfg!(debug_assertions) {
            tracing::warn!(
                "CORS permisivo: cualquier origen puede llamar a esta API con cualquier método \
                 y cabecera. Es una configuración de desarrollo, no de producción (RF-091)."
            );
        }
        Self {
            origins: Vec::new(),
            wildcard: true,
            reflect: true,
            allow_methods: join_methods(&SAFELISTED_METHODS),
            allow_headers: join_names(&safelisted_headers()),
            expose_headers: None,
            credentials: false,
            max_age: None,
        }
    }

    /// Lee la sección `[cors]` del `rdd.toml` del directorio actual (RF-093).
    ///
    /// Sin archivo o sin sección el resultado es el de RF-090: denegar todo. Un
    /// archivo que existe pero no parsea sí es un error, porque ahí había una
    /// intención de configurar que no se puede cumplir.
    pub fn from_config() -> Result<Self, CorsConfigError> {
        config::builder(Path::new("rdd.toml"))?.build()
    }

    fn allows(&self, origin: &str) -> bool {
        self.wildcard || self.origins.iter().any(|allowed| allowed == origin)
    }

    /// `*` solo si no hay credenciales; con credenciales el origen se refleja,
    /// que es lo único que el navegador acepta (RF-097).
    fn origin_value(&self, origin: &HeaderValue) -> HeaderValue {
        if self.wildcard && !self.credentials {
            HeaderValue::from_static("*")
        } else {
            origin.clone()
        }
    }

    /// RF-096: responde el preflight sin llamar a `next` (RF-094).
    fn preflight(&self, req: &Request, origin: &HeaderValue, allowed: bool) -> Response {
        let mut response = Response::new(Bytes::new());
        *response.status_mut() = StatusCode::NO_CONTENT;
        let headers = response.headers_mut();
        add_vary(
            headers,
            &[
                ORIGIN,
                ACCESS_CONTROL_REQUEST_METHOD,
                ACCESS_CONTROL_REQUEST_HEADERS,
            ],
        );
        // RF-098: origen denegado = 204 sin ninguna cabecera `Access-Control-*`.
        // El bloqueo lo ejecuta el navegador; `Vary` ya quedó puesto por caché.
        if !allowed {
            return response;
        }

        headers.insert(ACCESS_CONTROL_ALLOW_ORIGIN, self.origin_value(origin));
        if self.credentials {
            headers.insert(
                ACCESS_CONTROL_ALLOW_CREDENTIALS,
                HeaderValue::from_static("true"),
            );
        }
        headers.insert(
            ACCESS_CONTROL_ALLOW_METHODS,
            self.reflected(req, ACCESS_CONTROL_REQUEST_METHOD, &self.allow_methods),
        );
        headers.insert(
            ACCESS_CONTROL_ALLOW_HEADERS,
            self.reflected(req, ACCESS_CONTROL_REQUEST_HEADERS, &self.allow_headers),
        );
        if let Some(max_age) = &self.max_age {
            headers.insert(ACCESS_CONTROL_MAX_AGE, max_age.clone());
        }
        response
    }

    /// En modo permisivo el preflight anuncia lo que el cliente pidió; en modo
    /// configurado anuncia la lista validada, aunque el cliente pida más.
    fn reflected(
        &self,
        req: &Request,
        request_header: HeaderName,
        configured: &HeaderValue,
    ) -> HeaderValue {
        if self.reflect {
            req.headers()
                .get(request_header)
                .cloned()
                .unwrap_or_else(|| configured.clone())
        } else {
            configured.clone()
        }
    }

    /// RF-097: cabeceras de una petición normal con `Origin`.
    fn decorate(&self, origin: &HeaderValue, allowed: bool, headers: &mut HeaderMap) {
        // RF-098: `Vary: Origin` también al denegado, para que una caché no sirva
        // la respuesta denegada a un origen que sí estaba permitido.
        add_vary(headers, &[ORIGIN]);
        if !allowed {
            return;
        }
        headers.insert(ACCESS_CONTROL_ALLOW_ORIGIN, self.origin_value(origin));
        if self.credentials {
            headers.insert(
                ACCESS_CONTROL_ALLOW_CREDENTIALS,
                HeaderValue::from_static("true"),
            );
        }
        if let Some(expose) = &self.expose_headers {
            headers.insert(ACCESS_CONTROL_EXPOSE_HEADERS, expose.clone());
        }
    }
}

impl Middleware for Cors {
    fn handle<'a>(
        &'a self,
        req: Request,
        next: NextArc,
    ) -> Pin<Box<dyn Future<Output = rdd_core::Result<Response>> + Send + 'a>> {
        Box::pin(async move {
            // RF-099: sin `Origin` la petición no es CORS y pasa intacta.
            let Some(origin) = req.headers().get(ORIGIN).cloned() else {
                return next(req).await;
            };
            // Un `Origin` que no es texto no puede coincidir con ningún origen
            // configurado: se trata como denegado, no como petición sin CORS.
            let allowed = origin.to_str().is_ok_and(|value| self.allows(value));

            if req.method() == Method::OPTIONS
                && req.headers().contains_key(ACCESS_CONTROL_REQUEST_METHOD)
            {
                return Ok(self.preflight(&req, &origin, allowed));
            }

            let mut response = next(req).await?;
            self.decorate(&origin, allowed, response.headers_mut());
            Ok(response)
        })
    }
}

/// Builder de [`Cors`]: acumula sin validar y rechaza en [`build`](Self::build).
///
/// Separarlo del middleware validado permite encadenar los setters sin un `?`
/// por línea y, a la vez, rechazar en la construcción la combinación que el
/// estándar prohíbe (RF-092).
#[derive(Debug, Clone, Default)]
pub struct CorsBuilder {
    origins: Vec<String>,
    wildcard: bool,
    methods: Vec<Method>,
    headers: Vec<HeaderName>,
    expose: Vec<HeaderName>,
    credentials: bool,
    max_age: Option<Duration>,
    errors: Vec<CorsConfigError>,
}

impl CorsBuilder {
    /// Añade un origen permitido. Repetible: cada llamada suma a la lista.
    ///
    /// `"*"` activa el comodín, que [`build`](Self::build) rechaza si además hay
    /// credenciales (RF-092).
    pub fn allow_origin(mut self, origin: impl Into<String>) -> Self {
        let origin = origin.into();
        if origin == "*" {
            self.wildcard = true;
        } else if origin.is_empty() || HeaderValue::from_str(&origin).is_err() {
            self.errors
                .push(CorsConfigError::InvalidOrigin { value: origin });
        } else {
            self.origins.push(origin);
        }
        self
    }

    /// Métodos permitidos. Acepta `Method` o texto; sin llamarlo, el preflight
    /// anuncia los safelisted del estándar Fetch (§11.5).
    pub fn allow_methods<I, T>(mut self, methods: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: Display,
    {
        for method in methods {
            let text = method.to_string();
            match Method::from_str(&text) {
                Ok(method) => self.methods.push(method),
                Err(_) => self
                    .errors
                    .push(CorsConfigError::InvalidMethod { value: text }),
            }
        }
        self
    }

    /// Cabeceras de petición permitidas. Acepta `HeaderName` o texto.
    pub fn allow_headers<I, T>(mut self, headers: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: Display,
    {
        self.headers = parse_names(headers, &mut self.errors);
        self
    }

    /// Cabeceras de respuesta que el navegador puede leer (RF-097).
    pub fn expose_headers<I, T>(mut self, headers: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: Display,
    {
        self.expose = parse_names(headers, &mut self.errors);
        self
    }

    /// Permite cookies y cabeceras de autenticación en peticiones cruzadas.
    /// Con el comodín `*` es un error de configuración (RF-092).
    pub fn allow_credentials(mut self, allow: bool) -> Self {
        self.credentials = allow;
        self
    }

    /// Cuánto tiempo puede cachear el navegador el preflight. Sin llamarlo, no se
    /// emite `Access-Control-Max-Age` y el navegador usa su propio valor.
    pub fn max_age(mut self, age: Duration) -> Self {
        self.max_age = Some(age);
        self
    }

    /// Valida lo acumulado y produce el middleware (RF-090, RF-092).
    pub fn build(self) -> Result<Cors, CorsConfigError> {
        if let Some(error) = self.errors.first() {
            return Err(error.clone());
        }
        if self.credentials && self.wildcard {
            return Err(CorsConfigError::CredentialsWithWildcard);
        }

        let methods = if self.methods.is_empty() {
            SAFELISTED_METHODS.to_vec()
        } else {
            self.methods
        };
        let headers = if self.headers.is_empty() {
            safelisted_headers()
        } else {
            self.headers
        };

        Ok(Cors {
            origins: self.origins,
            wildcard: self.wildcard,
            reflect: false,
            allow_methods: join_methods(&methods),
            allow_headers: join_names(&headers),
            expose_headers: (!self.expose.is_empty()).then(|| join_names(&self.expose)),
            credentials: self.credentials,
            max_age: self.max_age.map(|age| {
                HeaderValue::from_str(&age.as_secs().to_string())
                    .expect("los segundos de un Duration son dígitos ASCII")
            }),
        })
    }
}

/// Error de configuración de [`Cors`].
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum CorsConfigError {
    /// El estándar Fetch prohíbe `*` con credenciales.
    #[error(
        "allow_credentials(true) no puede combinarse con el origen comodín `*`: \
         lo prohíbe el estándar Fetch (RF-092)"
    )]
    CredentialsWithWildcard,
    /// Un valor de `allow_methods` no parsea como método HTTP.
    #[error("`{value}` no es un método HTTP válido")]
    InvalidMethod {
        /// El valor tal como llegó, para que el error lo nombre.
        value: String,
    },
    /// Un valor de `allow_headers` o `expose_headers` no parsea como cabecera.
    #[error("`{value}` no es un nombre de cabecera válido")]
    InvalidHeader {
        /// El valor tal como llegó, para que el error lo nombre.
        value: String,
    },
    /// Un origen no puede ser valor de cabecera.
    #[error("`{value}` no es un origen válido")]
    InvalidOrigin {
        /// El valor tal como llegó, para que el error lo nombre.
        value: String,
    },
    /// El `rdd.toml` existe pero no se pudo leer o no parsea.
    #[error("no se pudo leer la configuración CORS: {0}")]
    Config(String),
}

fn safelisted_headers() -> Vec<HeaderName> {
    SAFELISTED_HEADERS
        .into_iter()
        .map(HeaderName::from_static)
        .collect()
}

fn parse_names<I, T>(names: I, errors: &mut Vec<CorsConfigError>) -> Vec<HeaderName>
where
    I: IntoIterator<Item = T>,
    T: Display,
{
    let mut parsed = Vec::new();
    for name in names {
        let text = name.to_string();
        match HeaderName::from_str(&text) {
            Ok(name) => parsed.push(name),
            Err(_) => errors.push(CorsConfigError::InvalidHeader { value: text }),
        }
    }
    parsed
}

/// Los valores se precalculan al construir: en petición solo se clonan, así que
/// el middleware no tiene ningún camino de error de conversión.
fn join_methods(methods: &[Method]) -> HeaderValue {
    let list = methods
        .iter()
        .map(Method::as_str)
        .collect::<Vec<_>>()
        .join(", ");
    HeaderValue::from_str(&list).expect("un Method es un token HTTP, siempre válido como cabecera")
}

fn join_names(names: &[HeaderName]) -> HeaderValue {
    let list = names
        .iter()
        .map(HeaderName::as_str)
        .collect::<Vec<_>>()
        .join(", ");
    HeaderValue::from_str(&list)
        .expect("un HeaderName es un token HTTP, siempre válido como cabecera")
}

/// Añade nombres a `Vary` sin pisar los que ya puso el handler.
///
/// Un `Vary` existente que no es texto no se puede reescribir sin perder bytes,
/// así que en ese caso se añade un campo nuevo: RFC 9110 los combina.
fn add_vary(headers: &mut HeaderMap, names: &[HeaderName]) {
    let mut existing: Vec<String> = Vec::new();
    let mut opaque = false;
    for value in headers.get_all(VARY) {
        match value.to_str() {
            Ok(text) => existing.extend(
                text.split(',')
                    .map(str::trim)
                    .filter(|part| !part.is_empty())
                    .map(str::to_owned),
            ),
            Err(_) => opaque = true,
        }
    }

    let mut added: Vec<&str> = Vec::new();
    for name in names {
        let text = name.as_str();
        let present =
            existing.iter().any(|part| part.eq_ignore_ascii_case(text)) || added.contains(&text);
        if !present {
            added.push(text);
        }
    }
    if added.is_empty() {
        return;
    }

    if opaque {
        headers.append(
            VARY,
            HeaderValue::from_str(&added.join(", "))
                .expect("los nombres de cabecera son tokens HTTP válidos"),
        );
        return;
    }
    existing.extend(added.iter().map(|name| (*name).to_owned()));
    headers.insert(
        VARY,
        HeaderValue::from_str(&existing.join(", "))
            .expect("Vary es una lista de tokens HTTP separados por comas"),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::header;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const APP: &str = "https://miapp.com";
    const OTHER: &str = "https://otrapp.com";

    fn request(method: Method, origin: Option<&str>) -> Request {
        let mut builder = http::Request::builder().method(method).uri("/tasks");
        if let Some(origin) = origin {
            builder = builder.header(ORIGIN, origin);
        }
        builder.body(Bytes::new()).expect("la petición es válida")
    }

    fn preflight(origin: &str, method: &str, headers: Option<&str>) -> Request {
        let mut builder = http::Request::builder()
            .method(Method::OPTIONS)
            .uri("/tasks")
            .header(ORIGIN, origin)
            .header(ACCESS_CONTROL_REQUEST_METHOD, method);
        if let Some(headers) = headers {
            builder = builder.header(ACCESS_CONTROL_REQUEST_HEADERS, headers);
        }
        builder.body(Bytes::new()).expect("la petición es válida")
    }

    fn next(calls: Arc<AtomicUsize>) -> NextArc {
        Arc::new(move |_req| {
            calls.fetch_add(1, Ordering::SeqCst);
            Box::pin(async {
                let mut response = Response::new(Bytes::from_static(b"[]"));
                response
                    .headers_mut()
                    .insert(VARY, HeaderValue::from_static("Accept"));
                Ok(response)
            })
        })
    }

    fn cors() -> Cors {
        Cors::new()
            .allow_origin(APP)
            .allow_methods([Method::GET, Method::POST, Method::DELETE])
            .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION])
            .expose_headers(["x-request-id"])
            .max_age(Duration::from_secs(600))
            .build()
            .expect("la configuración es válida")
    }

    fn vary(response: &Response) -> String {
        response
            .headers()
            .get_all(VARY)
            .iter()
            .map(|value| value.to_str().unwrap_or_default())
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// RF-090: sin orígenes configurados no se concede nada.
    #[tokio::test]
    async fn sin_configuracion_deniega_todo_origen() {
        let cors = Cors::new().build().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let response = cors
            .handle(request(Method::GET, Some(APP)), next(Arc::clone(&calls)))
            .await
            .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1, "RF-098: no se bloquea");
        assert!(!response.headers().contains_key(ACCESS_CONTROL_ALLOW_ORIGIN));
        assert_eq!(vary(&response), "Accept, origin");

        let pre = cors
            .handle(preflight(APP, "GET", None), next(calls))
            .await
            .unwrap();
        assert_eq!(pre.status(), StatusCode::NO_CONTENT);
        assert!(!pre.headers().contains_key(ACCESS_CONTROL_ALLOW_ORIGIN));
    }

    /// RF-096: el preflight completo, y RF-094: no ejecuta nada interno.
    #[tokio::test]
    async fn el_preflight_responde_204_con_todas_las_cabeceras_y_sin_llamar_a_next() {
        let calls = Arc::new(AtomicUsize::new(0));
        let response = cors()
            .handle(
                preflight(APP, "DELETE", Some("authorization, content-type")),
                next(Arc::clone(&calls)),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        assert!(response.body().is_empty());
        assert_eq!(calls.load(Ordering::SeqCst), 0, "RF-094");
        let headers = response.headers();
        assert_eq!(headers[ACCESS_CONTROL_ALLOW_ORIGIN], APP);
        assert_eq!(headers[ACCESS_CONTROL_ALLOW_METHODS], "GET, POST, DELETE");
        assert_eq!(
            headers[ACCESS_CONTROL_ALLOW_HEADERS],
            "content-type, authorization"
        );
        assert_eq!(headers[ACCESS_CONTROL_MAX_AGE], "600");
        assert!(!headers.contains_key(ACCESS_CONTROL_ALLOW_CREDENTIALS));
        assert_eq!(
            vary(&response),
            "origin, access-control-request-method, access-control-request-headers"
        );
    }

    /// RF-097: petición normal, con `Vary` del handler conservado.
    #[tokio::test]
    async fn la_peticion_normal_lleva_allow_origin_expose_y_vary() {
        let calls = Arc::new(AtomicUsize::new(0));
        let response = cors()
            .handle(request(Method::GET, Some(APP)), next(Arc::clone(&calls)))
            .await
            .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(response.headers()[ACCESS_CONTROL_ALLOW_ORIGIN], APP);
        assert_eq!(
            response.headers()[ACCESS_CONTROL_EXPOSE_HEADERS],
            "x-request-id"
        );
        assert_eq!(vary(&response), "Accept, origin");
        assert_eq!(response.body(), &Bytes::from_static(b"[]"));
    }

    /// RF-098: origen denegado no recibe cabeceras CORS pero sí pasa al handler.
    #[tokio::test]
    async fn un_origen_no_permitido_pasa_sin_cabeceras_cors() {
        let calls = Arc::new(AtomicUsize::new(0));
        let response = cors()
            .handle(request(Method::GET, Some(OTHER)), next(Arc::clone(&calls)))
            .await
            .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(response.status(), StatusCode::OK);
        for name in [
            ACCESS_CONTROL_ALLOW_ORIGIN,
            ACCESS_CONTROL_ALLOW_CREDENTIALS,
            ACCESS_CONTROL_EXPOSE_HEADERS,
        ] {
            assert!(!response.headers().contains_key(&name), "{name}");
        }

        let pre = cors()
            .handle(preflight(OTHER, "GET", None), next(calls))
            .await
            .unwrap();
        assert_eq!(pre.status(), StatusCode::NO_CONTENT);
        assert!(!pre.headers().contains_key(ACCESS_CONTROL_ALLOW_METHODS));
    }

    /// RF-099: sin `Origin` la respuesta queda exactamente como la del handler.
    #[tokio::test]
    async fn sin_origin_la_peticion_pasa_intacta() {
        let response = cors()
            .handle(
                request(Method::GET, None),
                next(Arc::new(AtomicUsize::new(0))),
            )
            .await
            .unwrap();
        assert_eq!(vary(&response), "Accept");
        assert!(!response.headers().contains_key(ACCESS_CONTROL_ALLOW_ORIGIN));
    }

    /// RF-092: el comodín con credenciales se rechaza al construir.
    #[test]
    fn rechaza_comodin_con_credenciales() {
        let error = Cors::new()
            .allow_origin("*")
            .allow_credentials(true)
            .build()
            .unwrap_err();
        assert_eq!(error, CorsConfigError::CredentialsWithWildcard);
        assert!(error.to_string().contains("RF-092"));
    }

    /// RF-097: con credenciales el origen se refleja, nunca `*`.
    #[tokio::test]
    async fn con_credenciales_refleja_el_origen_y_no_usa_comodin() {
        let cors = Cors::new()
            .allow_origin(APP)
            .allow_credentials(true)
            .build()
            .unwrap();
        let response = cors
            .handle(
                request(Method::GET, Some(APP)),
                next(Arc::new(AtomicUsize::new(0))),
            )
            .await
            .unwrap();
        assert_eq!(response.headers()[ACCESS_CONTROL_ALLOW_ORIGIN], APP);
        assert_eq!(response.headers()[ACCESS_CONTROL_ALLOW_CREDENTIALS], "true");

        let pre = cors
            .handle(
                preflight(APP, "POST", None),
                next(Arc::new(AtomicUsize::new(0))),
            )
            .await
            .unwrap();
        assert_eq!(pre.headers()[ACCESS_CONTROL_ALLOW_CREDENTIALS], "true");
    }

    /// `permissive()` refleja lo que pida el preflight y responde `*`.
    #[tokio::test]
    async fn permisivo_refleja_metodo_y_cabeceras() {
        let cors = Cors::permissive();
        let response = cors
            .handle(
                preflight(OTHER, "PATCH", Some("x-lo-que-sea")),
                next(Arc::new(AtomicUsize::new(0))),
            )
            .await
            .unwrap();
        assert_eq!(response.headers()[ACCESS_CONTROL_ALLOW_ORIGIN], "*");
        assert_eq!(response.headers()[ACCESS_CONTROL_ALLOW_METHODS], "PATCH");
        assert_eq!(
            response.headers()[ACCESS_CONTROL_ALLOW_HEADERS],
            "x-lo-que-sea"
        );
        assert!(
            !response
                .headers()
                .contains_key(ACCESS_CONTROL_ALLOW_CREDENTIALS)
        );
    }

    /// §11.5: sin `allow_methods`/`allow_headers` se anuncian los safelisted.
    #[tokio::test]
    async fn sin_listas_anuncia_los_safelisted() {
        let cors = Cors::new().allow_origin(APP).build().unwrap();
        let response = cors
            .handle(
                preflight(APP, "DELETE", None),
                next(Arc::new(AtomicUsize::new(0))),
            )
            .await
            .unwrap();
        assert_eq!(
            response.headers()[ACCESS_CONTROL_ALLOW_METHODS],
            "GET, HEAD, POST"
        );
        assert_eq!(
            response.headers()[ACCESS_CONTROL_ALLOW_HEADERS],
            "accept, accept-language, content-language, content-type"
        );
        assert!(!response.headers().contains_key(ACCESS_CONTROL_MAX_AGE));
    }

    /// `allow_origin` es repetible y un `OPTIONS` sin la cabecera de preflight no
    /// se trata como preflight.
    #[tokio::test]
    async fn varios_origenes_y_options_sin_request_method() {
        let cors = Cors::new()
            .allow_origin(APP)
            .allow_origin(OTHER)
            .build()
            .unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let response = cors
            .handle(
                request(Method::OPTIONS, Some(OTHER)),
                next(Arc::clone(&calls)),
            )
            .await
            .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1, "no es preflight (RF-096)");
        assert_eq!(response.headers()[ACCESS_CONTROL_ALLOW_ORIGIN], OTHER);
    }

    #[test]
    fn nombra_el_valor_invalido() {
        let error = Cors::new()
            .allow_headers(["cabecera inválida"])
            .build()
            .unwrap_err();
        assert_eq!(
            error,
            CorsConfigError::InvalidHeader {
                value: "cabecera inválida".to_owned()
            }
        );

        let error = Cors::new()
            .allow_methods(["NO ES UN MÉTODO"])
            .build()
            .unwrap_err();
        assert!(matches!(error, CorsConfigError::InvalidMethod { .. }));

        let error = Cors::new().allow_origin("").build().unwrap_err();
        assert!(matches!(error, CorsConfigError::InvalidOrigin { .. }));
    }

    /// RF-085/C-09: el preflight se responde antes de que un middleware interno
    /// que rechaza todo llegue a ejecutarse.
    #[tokio::test]
    async fn el_preflight_no_pasa_por_la_autenticacion() {
        let auth = rdd_core::http::fn_middleware(|_req: Request, _next| async {
            Err::<Response, _>(rdd_core::Error::Unauthorized)
        });
        let handler: rdd_core::http::CoreHandler =
            Arc::new(|_req| Box::pin(async { Ok(Response::new(Bytes::new())) }));
        let middlewares: [Arc<dyn Middleware>; 2] = [Arc::new(cors()), Arc::new(auth)];
        let chain = rdd_core::http::compose(&middlewares, handler);

        let response = chain(preflight(APP, "GET", None)).await.unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        assert_eq!(response.headers()[ACCESS_CONTROL_ALLOW_ORIGIN], APP);

        let real = chain(request(Method::GET, Some(APP))).await;
        assert!(matches!(real, Err(rdd_core::Error::Unauthorized)));
    }
}
