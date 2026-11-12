//! `StaticFiles`: resolución, caché y respuesta de un directorio (§10.11).
//!
//! El crate es neutral: recibe una [`Request`] ya preparada por la cadena global
//! y devuelve una respuesta o `None` cuando no hay archivo; la plataforma decide
//! entonces el 404 canónico. El montaje en rutas HTTP lo aporta `rdd-axum` (RP-033).

use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use bytes::Bytes;
use rdd_core::error::Problem;
use rdd_core::http::{
    HeaderName, HeaderValue, Request, Response, StatusCode,
    header::{ACCEPT, CACHE_CONTROL, CONTENT_LENGTH, CONTENT_TYPE, ETAG, IF_NONE_MATCH},
    problem_response,
    response::error_response_for,
};

use crate::config::StaticFilesConfig;
use crate::path;

const NOSNIFF: HeaderName = HeaderName::from_static("x-content-type-options");

/// Un directorio servido como archivos estáticos.
///
/// La raíz se canonicaliza al construir: un directorio inexistente es un error
/// de arranque, no un 404 en cada petición.
pub struct StaticFiles {
    root: PathBuf,
    config: StaticFilesConfig,
}

impl StaticFiles {
    /// Valida y canonicaliza la raíz (§10.11, RF-255).
    pub fn new(root: impl Into<PathBuf>) -> anyhow::Result<Self> {
        let requested = root.into();
        let root = std::fs::canonicalize(&requested).with_context(|| {
            format!(
                "el directorio estático `{}` no existe o no es accesible",
                requested.display()
            )
        })?;
        if !root.is_dir() {
            bail!(
                "el directorio estático `{}` no es una carpeta",
                root.display()
            );
        }
        let config = StaticFilesConfig::new();
        config.validate()?;
        Ok(Self { root, config })
    }

    /// Sustituye la configuración por defecto; también la valida.
    pub fn config(mut self, config: StaticFilesConfig) -> anyhow::Result<Self> {
        config.validate()?;
        self.config = config;
        Ok(self)
    }

    /// Configuración vigente, para que la plataforma valide el montaje.
    pub fn settings(&self) -> &StaticFilesConfig {
        &self.config
    }

    /// Raíz canonicalizada del montaje.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// `req` ya pasó por `HttpApp::prepare` (RP-028, RP-033).
    ///
    /// `None` significa que no hay archivo ni índice de SPA: la plataforma
    /// responde el 404 canónico del núcleo.
    pub async fn handle(&self, req: &Request) -> Option<Response> {
        let instance = req.uri().path();

        if let Some(authorize) = self.config.authorize_fn()
            && !self.config.is_public()
            && let Err(err) = authorize(req)
        {
            return Some(error_response_for(&err, instance));
        }

        let head = match req.method().as_str() {
            "GET" => false,
            "HEAD" => true,
            _ => return Some(method_not_allowed(instance)),
        };

        let decoded = path::decode(instance)?;
        let relative = path::relative(&decoded, self.config.mount_path())?;
        let wants_html = req
            .headers()
            .get(ACCEPT)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.contains("text/html"));

        let (file, served_relative, is_index, metadata) =
            self.resolve(relative, wants_html).await?;

        let etag = path::etag(&metadata);
        let cache = self.cache_control(&served_relative, is_index);
        let if_none_match = req
            .headers()
            .get(IF_NONE_MATCH)
            .and_then(|value| value.to_str().ok());
        if path::not_modified(if_none_match, &etag) {
            return Some(not_modified_response(&etag, cache));
        }

        let content_type = path::content_type(&file);
        let length = metadata.len();
        let body = if head {
            Bytes::new()
        } else {
            match tokio::fs::read(&file).await {
                Ok(bytes) => Bytes::from(bytes),
                Err(err) => {
                    let err = rdd_core::error::Error::internal(anyhow::Error::new(err));
                    return Some(error_response_for(&err, instance));
                }
            }
        };

        Some(file_response(body, length, &content_type, &etag, cache))
    }

    /// Archivo a servir para la ruta relativa, con su metadato.
    async fn resolve(
        &self,
        relative: &str,
        wants_html: bool,
    ) -> Option<(PathBuf, String, bool, std::fs::Metadata)> {
        let segments = path::segments(relative, self.config.dotfiles_enabled())?;

        let mut candidate = self.root.clone();
        for segment in &segments {
            candidate.push(segment);
        }

        if let Some(found) = self.file_under_root(&candidate).await {
            return Some((found.0, relative.to_owned(), false, found.1));
        }

        // `/` (o `/site`) resuelve al índice; una ruta como `/dashboard` busca
        // `/dashboard/index.html` antes de considerar el fallback de SPA.
        let index = candidate.join(self.config.index_file());
        if let Some(found) = self.file_under_root(&index).await {
            let served = if relative.is_empty() {
                self.config.index_file().to_owned()
            } else {
                format!(
                    "{}/{}",
                    relative.trim_end_matches('/'),
                    self.config.index_file()
                )
            };
            return Some((found.0, served, true, found.1));
        }

        if self.config.spa_enabled() && wants_html && !path::looks_like_file(relative) {
            let index = self.root.join(self.config.index_file());
            if let Some(found) = self.file_under_root(&index).await {
                return Some((found.0, self.config.index_file().to_owned(), true, found.1));
            }
        }
        None
    }

    /// `true` solo si existe, es archivo y su ruta real sigue bajo la raíz.
    async fn file_under_root(&self, candidate: &Path) -> Option<(PathBuf, std::fs::Metadata)> {
        let metadata = tokio::fs::metadata(candidate).await.ok()?;
        if !metadata.is_file() {
            return None;
        }
        let canonical = tokio::fs::canonicalize(candidate).await.ok()?;
        if !canonical.starts_with(&self.root) {
            return None;
        }
        Some((canonical, metadata))
    }

    fn cache_control(&self, relative: &str, is_index: bool) -> &'static str {
        if is_index {
            return "no-cache";
        }
        let relative = relative.trim_start_matches('/');
        let immutable = self.config.immutable_prefixes().iter().any(|prefix| {
            let prefix = prefix.trim_matches('/');
            !prefix.is_empty()
                && (relative == prefix || relative.starts_with(&format!("{prefix}/")))
        });
        if immutable {
            "public, max-age=31536000, immutable"
        } else {
            "no-cache"
        }
    }
}

fn file_response(
    body: Bytes,
    length: u64,
    content_type: &str,
    etag: &str,
    cache: &'static str,
) -> Response {
    let mut resp = Response::new(body);
    *resp.status_mut() = StatusCode::OK;
    let headers = resp.headers_mut();
    insert(headers, CONTENT_TYPE, content_type);
    insert(headers, CONTENT_LENGTH, &length.to_string());
    insert(headers, ETAG, etag);
    insert(headers, CACHE_CONTROL, cache);
    insert(headers, NOSNIFF, "nosniff");
    resp
}

fn not_modified_response(etag: &str, cache: &'static str) -> Response {
    let mut resp = Response::new(Bytes::new());
    *resp.status_mut() = StatusCode::NOT_MODIFIED;
    let headers = resp.headers_mut();
    insert(headers, ETAG, etag);
    insert(headers, CACHE_CONTROL, cache);
    insert(headers, NOSNIFF, "nosniff");
    resp
}

fn method_not_allowed(instance: &str) -> Response {
    let problem = Problem {
        r#type: "about:blank".to_owned(),
        title: "Method Not Allowed".to_owned(),
        status: StatusCode::METHOD_NOT_ALLOWED.as_u16(),
        detail: "los archivos estáticos solo admiten GET y HEAD".to_owned(),
        instance: instance.to_owned(),
    };
    problem_response(
        StatusCode::METHOD_NOT_ALLOWED,
        &problem,
        Some("GET, HEAD".to_owned()),
    )
}

fn insert(headers: &mut rdd_core::http::HeaderMap, name: HeaderName, value: &str) {
    if let Ok(value) = HeaderValue::from_str(value) {
        headers.insert(name, value);
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use rdd_core::http::{Method, StatusCode, header};

    use super::*;

    fn site() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        write(dir.path(), "index.html", "<h1>hola</h1>");
        write(dir.path(), "assets/app.js", "console.log(1);");
        write(dir.path(), "assets/app.css", "body{}");
        write(dir.path(), "secret.txt", "no");
        write(dir.path(), ".env", "secreto");
        dir
    }

    fn write(root: &Path, relative: &str, content: &str) {
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().expect("padre")).expect("mkdir");
        let mut file = std::fs::File::create(path).expect("archivo");
        file.write_all(content.as_bytes()).expect("escribir");
    }

    fn files(dir: &tempfile::TempDir, config: StaticFilesConfig) -> StaticFiles {
        StaticFiles::new(dir.path())
            .expect("raíz")
            .config(config)
            .expect("config")
    }

    fn request(method: Method, uri: &str) -> Request {
        let mut req = Request::new(Bytes::new());
        *req.method_mut() = method;
        *req.uri_mut() = uri.parse().expect("uri");
        req
    }

    #[tokio::test]
    async fn sirve_el_indice_con_cache_y_nosniff() {
        let dir = site();
        let files = files(&dir, StaticFilesConfig::new());
        let resp = files
            .handle(&request(Method::GET, "/"))
            .await
            .expect("índice");
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers()[header::CONTENT_TYPE],
            "text/html; charset=utf-8"
        );
        assert_eq!(resp.headers()[header::CACHE_CONTROL], "no-cache");
        assert_eq!(resp.headers()["x-content-type-options"], "nosniff");
        assert!(resp.headers().contains_key(header::ETAG));
        assert_eq!(resp.body().as_ref(), b"<h1>hola</h1>");
    }

    #[tokio::test]
    async fn sirve_assets_con_mime_y_cache_inmutable() {
        let dir = site();
        let files = files(&dir, StaticFilesConfig::new());
        let resp = files
            .handle(&request(Method::GET, "/assets/app.js"))
            .await
            .expect("asset");
        assert_eq!(
            resp.headers()[header::CONTENT_TYPE],
            "text/javascript; charset=utf-8"
        );
        assert_eq!(
            resp.headers()[header::CACHE_CONTROL],
            "public, max-age=31536000, immutable"
        );
    }

    #[tokio::test]
    async fn head_devuelve_longitud_sin_cuerpo() {
        let dir = site();
        let files = files(&dir, StaticFilesConfig::new());
        let resp = files
            .handle(&request(Method::HEAD, "/assets/app.js"))
            .await
            .expect("head");
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(resp.headers()[header::CONTENT_LENGTH], "15");
        assert!(resp.body().is_empty());
    }

    #[tokio::test]
    async fn etag_repetido_responde_304() {
        let dir = site();
        let files = files(&dir, StaticFilesConfig::new());
        let first = files
            .handle(&request(Method::GET, "/index.html"))
            .await
            .expect("primera");
        let etag = first.headers()[header::ETAG]
            .to_str()
            .expect("etag")
            .to_owned();

        let mut req = request(Method::GET, "/index.html");
        req.headers_mut().insert(
            header::IF_NONE_MATCH,
            HeaderValue::from_str(&etag).expect("valor"),
        );
        let second = files.handle(&req).await.expect("segunda");
        assert_eq!(second.status(), StatusCode::NOT_MODIFIED);
        assert!(second.body().is_empty());
    }

    #[tokio::test]
    async fn traversal_y_dotfiles_no_salen_de_la_raiz() {
        let dir = site();
        let files = files(&dir, StaticFilesConfig::new());
        for uri in [
            "/../secret.txt",
            "/%2e%2e/secret.txt",
            "/assets/%2e%2e/secret.txt",
            "/assets\\..\\secret.txt",
            "/.env",
        ] {
            assert!(
                files.handle(&request(Method::GET, uri)).await.is_none(),
                "{uri} no debe servirse"
            );
        }
    }

    #[tokio::test]
    async fn dotfiles_activos_sirven_el_archivo() {
        let dir = site();
        let files = files(&dir, StaticFilesConfig::new().dotfiles(true));
        let resp = files
            .handle(&request(Method::GET, "/.env"))
            .await
            .expect("dotfile");
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn solo_get_y_head() {
        let dir = site();
        let files = files(&dir, StaticFilesConfig::new());
        let resp = files
            .handle(&request(Method::POST, "/index.html"))
            .await
            .expect("post");
        assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
        assert_eq!(resp.headers()[header::ALLOW], "GET, HEAD");
    }

    #[tokio::test]
    async fn spa_sirve_el_indice_solo_sin_extension() {
        let dir = site();
        let files = files(&dir, StaticFilesConfig::new().spa(true));

        let mut req = request(Method::GET, "/dashboard");
        req.headers_mut()
            .insert(header::ACCEPT, HeaderValue::from_static("text/html"));
        let resp = files.handle(&req).await.expect("spa");
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(resp.body().as_ref(), b"<h1>hola</h1>");

        assert!(
            files
                .handle(&request(Method::GET, "/missing.js"))
                .await
                .is_none()
        );
        // Sin `Accept: text/html` no se inventa una página.
        assert!(
            files
                .handle(&request(Method::GET, "/dashboard"))
                .await
                .is_none()
        );
    }

    #[tokio::test]
    async fn authorize_precede_a_la_respuesta() {
        let dir = site();
        let files = files(
            &dir,
            StaticFilesConfig::new().authorize(|_| Err(rdd_core::Error::not_found("sin sesión"))),
        );
        let resp = files
            .handle(&request(Method::GET, "/index.html"))
            .await
            .expect("autorizado");
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);

        let public = files_with_public(&dir);
        assert_eq!(
            public
                .handle(&request(Method::GET, "/index.html"))
                .await
                .expect("público")
                .status(),
            StatusCode::OK
        );
    }

    fn files_with_public(dir: &tempfile::TempDir) -> StaticFiles {
        files(
            dir,
            StaticFilesConfig::new()
                .public(true)
                .authorize(|_| Err(rdd_core::Error::not_found("sin sesión"))),
        )
    }

    #[tokio::test]
    async fn montaje_con_prefijo() {
        let dir = site();
        let files = files(&dir, StaticFilesConfig::new().mount("/site"));
        assert!(
            files
                .handle(&request(Method::GET, "/site/assets/app.js"))
                .await
                .is_some()
        );
        assert!(
            files
                .handle(&request(Method::GET, "/assets/app.js"))
                .await
                .is_none()
        );
    }

    #[test]
    fn configuracion_invalida_falla() {
        assert!(StaticFilesConfig::new().mount("site").validate().is_err());
        assert!(StaticFilesConfig::new().mount("/site/").validate().is_err());
        assert!(
            StaticFilesConfig::new()
                .index("../index.html")
                .validate()
                .is_err()
        );
        assert!(
            StaticFilesConfig::new()
                .immutable("assets")
                .validate()
                .is_err()
        );
    }
}
