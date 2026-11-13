//! Adaptador de Google Cloud Storage con la API XML y V4 (§10.13, RF-274).
//!
//! Usa una clave HMAC de interoperabilidad y firma `GOOG4-HMAC-SHA256` con el
//! mismo armazón canónico que SigV4, en estilo path para que el mismo código
//! sirva contra `storage.googleapis.com` y `fake-gcs-server`.

use bytes::Bytes;
use chrono::{DateTime, Utc};
use http::Method;
use http::header::{self, HeaderMap};

use crate::endpoint::{self, Endpoint};
use crate::error::UploadConfigError;
use crate::http_client::{self, HttpClient, HttpRequest, HttpResponse};
use crate::key::ObjectKey;
use crate::meta::{ObjectMeta, PutOptions, StoredObject};
use crate::port::{BoxFuture, ObjectStorage};
use crate::secret::Secret;
use crate::sign;

/// Límite de objeto por defecto (64 MiB).
pub const DEFAULT_MAX_OBJECT_BYTES: usize = 64 * 1024 * 1024;
/// Endpoint público de la API XML.
pub const DEFAULT_ENDPOINT: &str = "https://storage.googleapis.com";

/// Configuración de GCS, por constructor o desde el entorno.
#[derive(Debug, Clone)]
pub struct GcsConfig {
    bucket: String,
    access_id: String,
    secret: Secret,
    endpoint: Option<String>,
    max_object_bytes: Option<usize>,
}

impl GcsConfig {
    /// Configuración mínima: bucket y clave HMAC de interoperabilidad.
    pub fn new(
        bucket: impl Into<String>,
        access_id: impl Into<String>,
        secret: impl Into<String>,
    ) -> Self {
        Self {
            bucket: bucket.into(),
            access_id: access_id.into(),
            secret: Secret::new(secret),
            endpoint: None,
            max_object_bytes: None,
        }
    }

    /// Endpoint alternativo (`fake-gcs-server`, MinIO en modo GCS).
    pub fn endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = Some(endpoint.into());
        self
    }

    /// Límite de objeto aceptado por el adaptador.
    pub fn max_object_bytes(mut self, max: usize) -> Self {
        self.max_object_bytes = Some(max);
        self
    }

    /// Bucket configurado.
    pub fn bucket(&self) -> &str {
        &self.bucket
    }

    /// Endpoint configurado, si lo hay.
    pub fn endpoint_value(&self) -> Option<&str> {
        self.endpoint.as_deref()
    }

    /// Límite efectivo.
    pub fn max_bytes(&self) -> usize {
        self.max_object_bytes.unwrap_or(DEFAULT_MAX_OBJECT_BYTES)
    }

    /// Lee `GCS_*` del entorno del proceso.
    pub fn from_env() -> Result<Self, UploadConfigError> {
        Self::from_lookup(|name| std::env::var(name).ok().filter(|value| !value.is_empty()))
    }

    /// Lee las mismas variables de una fuente inyectable (pruebas sin tocar el
    /// entorno del proceso, que en edición 2024 es `unsafe`).
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, UploadConfigError> {
        let required = |name| lookup(name).ok_or(UploadConfigError::Missing(name));
        let bucket = required("GCS_BUCKET")?;
        let access_id = required("GCS_HMAC_ACCESS_ID")?;
        let secret = required("GCS_HMAC_SECRET")?;
        let max_object_bytes = match lookup("GCS_MAX_OBJECT_BYTES") {
            Some(raw) => Some(endpoint::parse_usize("GCS_MAX_OBJECT_BYTES", &raw)?),
            None => None,
        };
        Ok(Self {
            bucket,
            access_id,
            secret: Secret::new(secret),
            endpoint: lookup("GCS_ENDPOINT"),
            max_object_bytes,
        })
    }
}

/// Storage de objetos sobre la API XML de GCS.
pub struct GcsStorage {
    config: GcsConfig,
    endpoint: Endpoint,
    max_bytes: usize,
    client: HttpClient,
}

impl GcsStorage {
    /// Valida la configuración y prepara el cliente HTTP.
    pub fn new(config: GcsConfig) -> Result<Self, UploadConfigError> {
        validate_bucket(&config.bucket)?;
        if config.access_id.is_empty() || config.secret.is_empty() {
            return Err(UploadConfigError::Config(
                "la clave HMAC de GCS no puede estar vacía".to_owned(),
            ));
        }
        let endpoint = match config.endpoint_value() {
            Some(raw) => Endpoint::parse(raw)?,
            None => Endpoint::parse(DEFAULT_ENDPOINT)?,
        };
        let max_bytes = config.max_bytes();
        if max_bytes == 0 {
            return Err(UploadConfigError::Config(
                "el límite de objeto debe ser mayor que cero".to_owned(),
            ));
        }
        let client = http_client::client()
            .map_err(|err| UploadConfigError::Config(format!("cliente HTTP: {err}")))?;
        Ok(Self {
            config,
            endpoint,
            max_bytes,
            client,
        })
    }

    /// Construye el adaptador leyendo `GCS_*` del entorno.
    pub fn from_env() -> Result<Self, UploadConfigError> {
        Self::new(GcsConfig::from_env()?)
    }

    /// Configuración efectiva.
    pub fn config(&self) -> &GcsConfig {
        &self.config
    }

    fn object_location(&self, key: &ObjectKey) -> (String, String, String) {
        let encoded = key.encoded_path();
        let base = self.endpoint.base_path();
        let path = if base.is_empty() {
            format!("/{}/{}", self.config.bucket, encoded)
        } else {
            format!("{base}/{}/{}", self.config.bucket, encoded)
        };
        (
            self.endpoint.url(&path),
            self.endpoint.host().to_owned(),
            path,
        )
    }

    fn signed_headers(
        &self,
        host: &str,
        payload_hash: &str,
        extras: &[(String, String)],
        now: DateTime<Utc>,
    ) -> Vec<(String, String)> {
        let date = now.format("%Y%m%dT%H%M%SZ").to_string();
        let mut headers = vec![
            ("host".to_owned(), host.to_owned()),
            ("x-goog-content-sha256".to_owned(), payload_hash.to_owned()),
            ("x-goog-date".to_owned(), date),
        ];
        headers.extend(extras.iter().cloned());
        headers
    }

    fn authorization(
        &self,
        method: &str,
        path: &str,
        headers: &[(String, String)],
        payload_hash: &str,
        now: DateTime<Utc>,
    ) -> String {
        let date = now.format("%Y%m%d").to_string();
        let (canonical, signed_headers) =
            sign::canonical_request(method, path, headers, payload_hash);
        let scope = format!("{date}/auto/storage/goog4_request");
        let string_to_sign = format!(
            "GOOG4-HMAC-SHA256\n{}\n{scope}\n{}",
            now.format("%Y%m%dT%H%M%SZ"),
            sign::sha256_hex(canonical.as_bytes())
        );
        let key = sign::gcs_signing_key(self.config.secret.expose(), &date);
        let signature = sign::hmac_sha256_hex(&key, string_to_sign.as_bytes());
        format!(
            "GOOG4-HMAC-SHA256 Credential={}/{scope}, SignedHeaders={signed_headers}, Signature={signature}",
            self.config.access_id
        )
    }

    fn extras(&self, options: &PutOptions) -> Vec<(String, String)> {
        let mut extras = Vec::new();
        if let Some(value) = &options.content_type {
            extras.push(("content-type".to_owned(), value.clone()));
        }
        if let Some(value) = &options.cache_control {
            extras.push(("cache-control".to_owned(), value.clone()));
        }
        for (name, value) in &options.metadata {
            extras.push((
                format!("x-goog-meta-{}", name.to_ascii_lowercase()),
                value.clone(),
            ));
        }
        extras
    }

    fn request(
        &self,
        method: Method,
        url: String,
        body: Bytes,
        headers: Vec<(String, String)>,
    ) -> rdd_core::Result<HttpRequest> {
        let mut request = HttpRequest::new(method, url).body(body);
        for (name, value) in headers {
            let (name, value) = http_client::header_pair(&name, &value, "cabecera firmada")?;
            request = request.header(name, value);
        }
        Ok(request)
    }
}

impl ObjectStorage for GcsStorage {
    fn put(
        &self,
        key: &ObjectKey,
        body: Bytes,
        options: PutOptions,
    ) -> BoxFuture<'_, rdd_core::Result<ObjectMeta>> {
        let key = key.clone();
        Box::pin(async move {
            if body.len() > self.max_bytes {
                return Err(oversize(body.len(), self.max_bytes));
            }
            let now = Utc::now();
            let payload_hash = sign::sha256_hex(&body);
            let (url, host, path) = self.object_location(&key);
            let extras = self.extras(&options);
            let mut headers = self.signed_headers(&host, &payload_hash, &extras, now);
            let authorization = self.authorization("PUT", &path, &headers, &payload_hash, now);
            headers.push(("authorization".to_owned(), authorization));
            let size = body.len() as u64;
            let request = self.request(Method::PUT, url, body, headers)?;
            let response = http_client::send(&self.client, request, self.max_bytes).await?;
            map_put(&key, &options, size, response)
        })
    }

    fn get(&self, key: &ObjectKey) -> BoxFuture<'_, rdd_core::Result<Option<StoredObject>>> {
        let key = key.clone();
        Box::pin(async move {
            let now = Utc::now();
            let payload_hash = sign::sha256_empty_hex();
            let (url, host, path) = self.object_location(&key);
            let mut headers = self.signed_headers(&host, &payload_hash, &[], now);
            let authorization = self.authorization("GET", &path, &headers, &payload_hash, now);
            headers.push(("authorization".to_owned(), authorization));
            let request = self.request(Method::GET, url, Bytes::new(), headers)?;
            let response = http_client::send(&self.client, request, self.max_bytes).await?;
            map_get(&key, response)
        })
    }

    fn head(&self, key: &ObjectKey) -> BoxFuture<'_, rdd_core::Result<Option<ObjectMeta>>> {
        let key = key.clone();
        Box::pin(async move {
            let now = Utc::now();
            let payload_hash = sign::sha256_empty_hex();
            let (url, host, path) = self.object_location(&key);
            let mut headers = self.signed_headers(&host, &payload_hash, &[], now);
            let authorization = self.authorization("HEAD", &path, &headers, &payload_hash, now);
            headers.push(("authorization".to_owned(), authorization));
            let request = self.request(Method::HEAD, url, Bytes::new(), headers)?;
            let response = http_client::send(&self.client, request, 64 * 1024).await?;
            map_head(&key, response)
        })
    }

    fn delete(&self, key: &ObjectKey) -> BoxFuture<'_, rdd_core::Result<()>> {
        let key = key.clone();
        Box::pin(async move {
            let now = Utc::now();
            let payload_hash = sign::sha256_empty_hex();
            let (url, host, path) = self.object_location(&key);
            let mut headers = self.signed_headers(&host, &payload_hash, &[], now);
            let authorization = self.authorization("DELETE", &path, &headers, &payload_hash, now);
            headers.push(("authorization".to_owned(), authorization));
            let request = self.request(Method::DELETE, url, Bytes::new(), headers)?;
            let response = http_client::send(&self.client, request, 64 * 1024).await?;
            map_delete(response)
        })
    }
}

fn map_put(
    key: &ObjectKey,
    options: &PutOptions,
    size: u64,
    response: HttpResponse,
) -> rdd_core::Result<ObjectMeta> {
    if !response.is_success() {
        return Err(http_client::provider_error("GCS", &response));
    }
    Ok(ObjectMeta {
        key: key.clone(),
        size,
        content_type: options.content_type.clone(),
        etag: response.header_str("etag").map(str::to_owned),
        last_modified: None,
    })
}

fn map_get(key: &ObjectKey, response: HttpResponse) -> rdd_core::Result<Option<StoredObject>> {
    if response.is_not_found() {
        return Ok(None);
    }
    if !response.is_success() {
        return Err(http_client::provider_error("GCS", &response));
    }
    let meta = meta_from_headers(key, &response.headers, response.body.len() as u64);
    Ok(Some(StoredObject {
        meta,
        body: response.body,
    }))
}

fn map_head(key: &ObjectKey, response: HttpResponse) -> rdd_core::Result<Option<ObjectMeta>> {
    if response.is_not_found() {
        return Ok(None);
    }
    if !response.is_success() {
        return Err(http_client::provider_error("GCS", &response));
    }
    let size = response
        .header_str("content-length")
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    Ok(Some(meta_from_headers(key, &response.headers, size)))
}

fn map_delete(response: HttpResponse) -> rdd_core::Result<()> {
    if response.is_success() || response.is_not_found() {
        return Ok(());
    }
    Err(http_client::provider_error("GCS", &response))
}

fn meta_from_headers(key: &ObjectKey, headers: &HeaderMap, size: u64) -> ObjectMeta {
    ObjectMeta {
        key: key.clone(),
        size,
        content_type: headers
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned),
        etag: headers
            .get(header::ETAG)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned),
        last_modified: headers
            .get(header::LAST_MODIFIED)
            .and_then(|value| value.to_str().ok())
            .and_then(http_client::parse_http_date),
    }
}

fn oversize(size: usize, max: usize) -> rdd_core::Error {
    rdd_core::Error::internal(anyhow::anyhow!(
        "el objeto mide {size} bytes y el límite es {max} (RF-274)"
    ))
}

fn validate_bucket(bucket: &str) -> Result<(), UploadConfigError> {
    let valid = (3..=63).contains(&bucket.len())
        && bucket.bytes().all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || byte == b'-'
                || byte == b'_'
                || byte == b'.'
        })
        && bucket
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_alphanumeric())
        && bucket
            .bytes()
            .last()
            .is_some_and(|byte| byte.is_ascii_alphanumeric());
    if valid {
        Ok(())
    } else {
        Err(UploadConfigError::Config(format!(
            "el bucket de GCS `{bucket}` no es válido: 3 a 63 caracteres en minúscula, dígitos, \
             `-`, `_` o `.`, empezando y terminando en letra o dígito"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ACCESS_ID: &str = "GOOG1EXAMPLEACCESSID";
    const SECRET: &str = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY";

    fn storage(endpoint: Option<&str>) -> GcsStorage {
        let mut config = GcsConfig::new("mi-bucket", ACCESS_ID, SECRET);
        if let Some(endpoint) = endpoint {
            config = config.endpoint(endpoint);
        }
        GcsStorage::new(config).unwrap()
    }

    fn key() -> ObjectKey {
        ObjectKey::parse("files/a/uno.txt").unwrap()
    }

    #[test]
    fn usa_estilo_path_y_el_endpoint_cambia_el_host_firmado() {
        let gcs = storage(None);
        let (url, host, path) = gcs.object_location(&key());
        assert_eq!(host, "storage.googleapis.com");
        assert_eq!(path, "/mi-bucket/files/a/uno.txt");
        assert_eq!(
            url,
            "https://storage.googleapis.com/mi-bucket/files/a/uno.txt"
        );

        let local = storage(Some("http://127.0.0.1:4443"));
        let (url, host, path) = local.object_location(&key());
        assert_eq!(host, "127.0.0.1:4443");
        assert_eq!(path, "/mi-bucket/files/a/uno.txt");
        assert_eq!(url, "http://127.0.0.1:4443/mi-bucket/files/a/uno.txt");
    }

    #[test]
    fn el_vector_v4_reproduce_goog4_hmac_sha256() {
        // La firma esperada se calculó con una implementación HMAC independiente
        // (OpenSSL/Node) sobre la petición canónica V4.
        let storage = storage(None);
        let now: DateTime<Utc> = "2026-11-13T00:00:00Z".parse().unwrap();
        let payload_hash = sign::sha256_hex(b"hola");
        assert_eq!(
            payload_hash,
            "b221d9dbb083a7f33428d7c2a3c3198ae925614d70210e28716ccaa7cd4ddb79"
        );
        let extras = vec![("content-type".to_owned(), "text/plain".to_owned())];
        let headers = storage.signed_headers("storage.googleapis.com", &payload_hash, &extras, now);
        let authorization = storage.authorization(
            "PUT",
            "/mi-bucket/files/a/uno.txt",
            &headers,
            &payload_hash,
            now,
        );
        assert_eq!(
            authorization,
            "GOOG4-HMAC-SHA256 Credential=GOOG1EXAMPLEACCESSID/20261113/auto/storage/goog4_request, \
             SignedHeaders=content-type;host;x-goog-content-sha256;x-goog-date, \
             Signature=37426914340beadc39317bd2b8bbe6f90a3c1cb9988c71d974ffa3209b95eb0b"
        );
    }

    #[test]
    fn put_firma_el_tipo_de_contenido_y_los_metadatos() {
        let storage = storage(None);
        let options = PutOptions::new()
            .content_type("text/plain")
            .cache_control("max-age=60")
            .metadata("origen", "demo");
        let extras = storage.extras(&options);
        assert!(
            extras
                .iter()
                .any(|(name, value)| name == "content-type" && value == "text/plain")
        );
        assert!(
            extras
                .iter()
                .any(|(name, value)| name == "cache-control" && value == "max-age=60")
        );
        assert!(
            extras
                .iter()
                .any(|(name, value)| name == "x-goog-meta-origen" && value == "demo")
        );
    }

    #[test]
    fn los_404_se_traducen_a_none_o_ok() {
        let not_found = |status| HttpResponse {
            status,
            headers: HeaderMap::new(),
            body: Bytes::new(),
        };
        assert!(
            map_get(&key(), not_found(http::StatusCode::NOT_FOUND))
                .unwrap()
                .is_none()
        );
        assert!(
            map_head(&key(), not_found(http::StatusCode::NOT_FOUND))
                .unwrap()
                .is_none()
        );
        assert!(map_delete(not_found(http::StatusCode::NOT_FOUND)).is_ok());
        assert!(map_delete(not_found(http::StatusCode::NO_CONTENT)).is_ok());
    }

    #[test]
    fn from_lookup_prefiere_el_constructor_y_nombra_la_variable_ausente() {
        let env = |name: &str| match name {
            "GCS_BUCKET" => Some("bucket-env".to_owned()),
            "GCS_HMAC_ACCESS_ID" => Some("id-env".to_owned()),
            "GCS_HMAC_SECRET" => Some("secreto-env".to_owned()),
            "GCS_ENDPOINT" => Some("http://127.0.0.1:4443".to_owned()),
            "GCS_MAX_OBJECT_BYTES" => Some("1024".to_owned()),
            _ => None,
        };
        let config = GcsConfig::from_lookup(env).unwrap();
        assert_eq!(config.bucket(), "bucket-env");
        assert_eq!(config.max_bytes(), 1024);
        assert_eq!(config.endpoint_value(), Some("http://127.0.0.1:4443"));
        let overridden = config.clone().max_object_bytes(2048);
        assert_eq!(overridden.max_bytes(), 2048);

        let err = GcsConfig::from_lookup(|name| {
            if name == "GCS_BUCKET" {
                Some("bucket".to_owned())
            } else {
                None
            }
        })
        .unwrap_err();
        assert!(err.to_string().contains("GCS_HMAC_ACCESS_ID"), "{err}");
    }
}
