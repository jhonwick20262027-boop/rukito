//! Semántica HTTP de la documentación OpenAPI (§10.10, RP-032).
//!
//! Neutral respecto del servidor: recibe una [`Request`] ya preparada por la
//! cadena global, decide entre la UI y el JSON y devuelve la respuesta. El
//! montaje lo hace `rdd-axum`.

use std::sync::Arc;

use bytes::Bytes;
use rdd_core::error::Problem;
use rdd_core::http::{
    HeaderName, HeaderValue, Request, Response, StatusCode,
    header::{CACHE_CONTROL, CONTENT_TYPE},
    normalize_path, problem_response,
    response::error_response_for,
};

use crate::{DEFAULT_ALIASES, DEFAULT_PATH, OpenApi};

/// UI compilada: un único HTML con JS y CSS inline (§10.10, ADR-31).
const DOCS: &str = include_str!("api-docs.html");
/// Marcador que el servidor sustituye por el documento serializado.
const DOCUMENT_MARKER: &str = "__RDD_OPENAPI_DOCUMENT__";
const CSP: &str = "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; \
                   connect-src 'self'";

/// Guardián ejecutado después de los globales (RF-249).
type AuthorizeFn = Arc<dyn Fn(&Request) -> rdd_core::Result<()> + Send + Sync>;

/// Configuración del montaje de la documentación (RF-250).
pub struct OpenApiHttpConfig {
    path: String,
    docs: bool,
    public_docs: bool,
    aliases: Vec<String>,
    strict_routes: bool,
    authorize: Option<AuthorizeFn>,
}

impl Default for OpenApiHttpConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl OpenApiHttpConfig {
    /// `/api-docs` con alias `/docs` y `/swagger`, UI activa y rutas estrictas.
    pub fn new() -> Self {
        Self {
            path: DEFAULT_PATH.to_owned(),
            docs: true,
            public_docs: false,
            aliases: DEFAULT_ALIASES.map(str::to_owned).to_vec(),
            strict_routes: true,
            authorize: None,
        }
    }

    /// Ruta canónica de la UI.
    pub fn path(mut self, path: impl Into<String>) -> Self {
        self.path = path.into();
        self
    }

    /// `false` desactiva la UI y el JSON (404 tras autorizar).
    pub fn docs(mut self, enabled: bool) -> Self {
        self.docs = enabled;
        self
    }

    /// Si es `true`, la documentación se sirve sin ejecutar `authorize`.
    pub fn public_docs(mut self, enabled: bool) -> Self {
        self.public_docs = enabled;
        self
    }

    /// Rutas adicionales que sirven la misma UI.
    pub fn aliases<I, S>(mut self, aliases: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.aliases = aliases.into_iter().map(Into::into).collect();
        self
    }

    /// `true` exige que toda ruta montada esté documentada (RF-248).
    pub fn strict_routes(mut self, enabled: bool) -> Self {
        self.strict_routes = enabled;
        self
    }

    /// Se ejecuta después de los globales; `Err` se responde como Problem Details.
    pub fn authorize<F>(mut self, f: F) -> Self
    where
        F: Fn(&Request) -> rdd_core::Result<()> + Send + Sync + 'static,
    {
        self.authorize = Some(Arc::new(f));
        self
    }

    /// Ruta canónica, para que la plataforma valide el montaje.
    pub fn mount_path(&self) -> &str {
        &self.path
    }

    /// Alias configurados.
    pub fn alias_paths(&self) -> &[String] {
        &self.aliases
    }

    /// Ruta canónica del JSON, `{path}/openapi.json`.
    pub fn json_path(&self) -> String {
        format!("{}/openapi.json", self.path.trim_end_matches('/'))
    }

    /// Si la documentación está habilitada.
    pub fn docs_enabled(&self) -> bool {
        self.docs
    }

    /// Si la documentación se sirve sin `authorize`.
    pub fn public_docs_enabled(&self) -> bool {
        self.public_docs
    }

    /// Si se exige que el documento cubra todas las rutas montadas.
    pub fn strict_routes_enabled(&self) -> bool {
        self.strict_routes
    }

    /// Rechaza rutas que no puedan montarse (RF-250).
    pub fn validate(&self) -> rdd_core::Result<()> {
        let mut seen: Vec<String> = Vec::new();
        for raw in std::iter::once(&self.path).chain(self.aliases.iter()) {
            let path = normalize_path(raw);
            let literal = raw.starts_with('/')
                && raw != "/"
                && raw == &path
                && !raw.contains([':', '*', '{', '}', '?', ' ', '\t', '\n']);
            if !literal {
                return Err(rdd_core::Error::internal(anyhow::anyhow!(
                    "ruta OpenAPI inválida `{raw}`: se espera una ruta literal como `/api-docs`"
                )));
            }
            if seen.contains(&path) {
                return Err(rdd_core::Error::internal(anyhow::anyhow!(
                    "ruta OpenAPI duplicada `{path}`"
                )));
            }
            seen.push(path);
        }
        let json = normalize_path(&self.json_path());
        if seen.contains(&json) {
            return Err(rdd_core::Error::internal(anyhow::anyhow!(
                "la ruta del JSON `{json}` choca con otra ruta de la documentación"
            )));
        }
        Ok(())
    }
}

/// Punto de entrada de la documentación sobre el mismo `HttpApp` (RF-245).
pub struct OpenApiHttp {
    doc: Arc<OpenApi>,
    config: OpenApiHttpConfig,
    json: Arc<String>,
    html: String,
}

impl OpenApiHttp {
    /// Monta el documento con la configuración por defecto.
    pub fn new(doc: OpenApi) -> Self {
        let json = serde_json::to_string(&doc).unwrap_or_else(|_| {
            "{\"error\":\"el documento OpenAPI no es serializable\"}".to_owned()
        });
        let escaped = json
            .replace('<', "\\u003c")
            .replace('\u{2028}', "\\u2028")
            .replace('\u{2029}', "\\u2029");
        let html = DOCS.replace(DOCUMENT_MARKER, &escaped);
        Self {
            doc: Arc::new(doc),
            config: OpenApiHttpConfig::new(),
            json: Arc::new(json),
            html,
        }
    }

    /// Sustituye la configuración por defecto.
    pub fn config(mut self, config: OpenApiHttpConfig) -> Self {
        self.config = config;
        self
    }

    /// Ruta canónica donde la plataforma debe montarlo.
    pub fn path(&self) -> &str {
        self.config.mount_path()
    }

    /// Configuración efectiva, para que la plataforma valide el montaje.
    pub fn settings(&self) -> &OpenApiHttpConfig {
        &self.config
    }

    /// Documento servido, para validarlo contra las rutas montadas.
    pub fn document(&self) -> &OpenApi {
        &self.doc
    }

    /// `req` ya pasó por `HttpApp::prepare` (RP-028).
    pub async fn handle(&self, req: Request) -> OpenApiHttpResponse {
        let instance = normalize_path(req.uri().path());

        if let Some(authorize) = &self.config.authorize
            && !self.config.public_docs
            && let Err(err) = authorize(&req)
        {
            return OpenApiHttpResponse::Json(no_store(error_response_for(&err, &instance)));
        }

        if req.method().as_str() != "GET" {
            return OpenApiHttpResponse::Json(no_store(problem(
                StatusCode::METHOD_NOT_ALLOWED,
                "Method Not Allowed",
                "La documentación solo admite GET",
                &instance,
                Some("GET"),
            )));
        }

        if !self.config.docs {
            return OpenApiHttpResponse::Json(no_store(problem(
                StatusCode::NOT_FOUND,
                "Not Found",
                "la documentación OpenAPI está desactivada",
                &instance,
                None,
            )));
        }

        let path = normalize_path(req.uri().path());
        let canonical = normalize_path(&self.config.path);
        if path == canonical
            || self
                .config
                .aliases
                .iter()
                .any(|alias| normalize_path(alias) == path)
        {
            return OpenApiHttpResponse::Html(no_store(html_response(&self.html)));
        }
        if path == normalize_path(&self.config.json_path()) {
            return OpenApiHttpResponse::Json(no_store(json_response(&self.json)));
        }

        OpenApiHttpResponse::Json(no_store(problem(
            StatusCode::NOT_FOUND,
            "Not Found",
            "ruta de documentación desconocida",
            &instance,
            None,
        )))
    }
}

/// Respuesta de la documentación; la plataforma solo traduce el cuerpo.
pub enum OpenApiHttpResponse {
    /// UI embebida.
    Html(Response),
    /// Documento JSON o Problem Details.
    Json(Response),
}

fn html_response(html: &str) -> Response {
    let mut resp = Response::new(Bytes::copy_from_slice(html.as_bytes()));
    resp.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    resp.headers_mut().insert(
        HeaderName::from_static("content-security-policy"),
        HeaderValue::from_static(CSP),
    );
    resp.headers_mut().insert(
        HeaderName::from_static("x-content-type-options"),
        HeaderValue::from_static("nosniff"),
    );
    resp
}

/// El JSON ya viene serializado; no se vuelve a serializar como cadena.
fn json_response(json: &str) -> Response {
    let mut resp = Response::new(Bytes::copy_from_slice(json.as_bytes()));
    resp.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static("application/json; charset=utf-8"),
    );
    resp.headers_mut().insert(
        HeaderName::from_static("x-content-type-options"),
        HeaderValue::from_static("nosniff"),
    );
    resp
}

fn no_store(mut resp: Response) -> Response {
    resp.headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    resp
}

fn problem(
    status: StatusCode,
    title: &str,
    detail: &str,
    instance: &str,
    allow: Option<&str>,
) -> Response {
    let body = Problem {
        r#type: "about:blank".to_owned(),
        title: title.to_owned(),
        status: status.as_u16(),
        detail: detail.to_owned(),
        instance: instance.to_owned(),
    };
    problem_response(status, &body, allow.map(str::to_owned))
}
