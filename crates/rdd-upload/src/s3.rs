//! Adaptador de AWS S3 con SigV4 escrito a mano (§10.13, RF-272).
//!
//! Sin SDK: una petición HTTP/1.1 firmada por objeto. Los cuerpos viajan en
//! memoria y un fallo del proveedor se devuelve tal cual, sin reintentos.

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
use crate::secret::Credentials;
use crate::sign;

/// Región por defecto cuando no se configura ninguna.
pub const DEFAULT_REGION: &str = "us-east-1";
/// Límite de objeto por defecto (64 MiB).
pub const DEFAULT_MAX_OBJECT_BYTES: usize = 64 * 1024 * 1024;

/// Configuración de S3, por constructor o desde el entorno.
#[derive(Debug, Clone)]
pub struct S3Config {
    bucket: String,
    credentials: Credentials,
    region: Option<String>,
    endpoint: Option<String>,
    force_path_style: Option<bool>,
    max_object_bytes: Option<usize>,
}

impl S3Config {
    /// Configuración mínima: bucket y credenciales estáticas.
    pub fn new(bucket: impl Into<String>, credentials: Credentials) -> Self {
        Self {
            bucket: bucket.into(),
            credentials,
            region: None,
            endpoint: None,
            force_path_style: None,
            max_object_bytes: None,
        }
    }

    /// Región de firma; por defecto `us-east-1`.
    pub fn region(mut self, region: impl Into<String>) -> Self {
        self.region = Some(region.into());
        self
    }

    /// Endpoint alternativo (MinIO, LocalStack); su presencia activa path-style.
    pub fn endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = Some(endpoint.into());
        self
    }

    /// Fuerza `/{bucket}/{key}` en lugar de virtual-host.
    pub fn force_path_style(mut self, force: bool) -> Self {
        self.force_path_style = Some(force);
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

    /// Región efectiva.
    pub fn region_value(&self) -> &str {
        self.region.as_deref().unwrap_or(DEFAULT_REGION)
    }

    /// Endpoint configurado, si lo hay.
    pub fn endpoint_value(&self) -> Option<&str> {
        self.endpoint.as_deref()
    }

    /// Path-style efectivo: explícito o, sin endpoint, virtual-host.
    pub fn uses_path_style(&self) -> bool {
        self.force_path_style.unwrap_or(self.endpoint.is_some())
    }

    /// Límite efectivo.
    pub fn max_bytes(&self) -> usize {
        self.max_object_bytes.unwrap_or(DEFAULT_MAX_OBJECT_BYTES)
    }

    /// Lee `AWS_*` del entorno del proceso.
    pub fn from_env() -> Result<Self, UploadConfigError> {
        Self::from_lookup(|name| std::env::var(name).ok().filter(|value| !value.is_empty()))
    }

    /// Lee las mismas variables de una fuente inyectable (pruebas sin tocar el
    /// entorno del proceso, que en edición 2024 es `unsafe`).
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, UploadConfigError> {
        let required = |name| lookup(name).ok_or(UploadConfigError::Missing(name));
        let bucket = required("AWS_S3_BUCKET")?;
        let access_key = required("AWS_ACCESS_KEY_ID")?;
        let secret_key = required("AWS_SECRET_ACCESS_KEY")?;
        let mut credentials = Credentials::new(access_key, secret_key);
        if let Some(token) = lookup("AWS_SESSION_TOKEN") {
            credentials = credentials.with_session_token(token);
        }
        let force_path_style = match lookup("AWS_S3_FORCE_PATH_STYLE") {
            Some(raw) => Some(endpoint::parse_bool("AWS_S3_FORCE_PATH_STYLE", &raw)?),
            None => None,
        };
        let max_object_bytes = match lookup("AWS_MAX_OBJECT_BYTES") {
            Some(raw) => Some(endpoint::parse_usize("AWS_MAX_OBJECT_BYTES", &raw)?),
            None => None,
        };
        Ok(Self {
            bucket,
            credentials,
            region: lookup("AWS_REGION"),
            endpoint: lookup("AWS_ENDPOINT_URL"),
            force_path_style,
            max_object_bytes,
        })
    }
}

/// Storage de objetos sobre la API REST de S3.
pub struct S3Storage {
    config: S3Config,
    region: String,
    endpoint: Endpoint,
    path_style: bool,
    max_bytes: usize,
    client: HttpClient,
}

impl S3Storage {
    /// Valida la configuración y prepara el cliente HTTP.
    pub fn new(config: S3Config) -> Result<Self, UploadConfigError> {
        validate_bucket(&config.bucket)?;
        if config.credentials.access_key().is_empty() || config.credentials.secret().is_empty() {
            return Err(UploadConfigError::Config(
                "las credenciales de AWS no pueden estar vacías".to_owned(),
            ));
        }
        let region = config.region_value().to_owned();
        validate_region(&region)?;
        let endpoint = match config.endpoint_value() {
            Some(raw) => Endpoint::parse(raw)?,
            None => Endpoint::aws_s3(&region)?,
        };
        let max_bytes = config.max_bytes();
        if max_bytes == 0 {
            return Err(UploadConfigError::Config(
                "el límite de objeto debe ser mayor que cero".to_owned(),
            ));
        }
        let path_style = config.uses_path_style();
        let client = http_client::client()
            .map_err(|err| UploadConfigError::Config(format!("cliente HTTP: {err}")))?;
        Ok(Self {
            config,
            region,
            endpoint,
            path_style,
            max_bytes,
            client,
        })
    }

    /// Construye el adaptador leyendo `AWS_*` del entorno.
    pub fn from_env() -> Result<Self, UploadConfigError> {
        Self::new(S3Config::from_env()?)
    }

    /// Configuración efectiva.
    pub fn config(&self) -> &S3Config {
        &self.config
    }

    fn object_location(&self, key: &ObjectKey) -> (String, String, String) {
        let encoded = key.encoded_path();
        let (host, path) = if self.path_style {
            (
                self.endpoint.host().to_owned(),
                format!(
                    "{}/{}/{}",
                    self.endpoint.base_path(),
                    self.config.bucket(),
                    encoded
                ),
            )
        } else {
            (
                format!("{}.{}", self.config.bucket(), self.endpoint.host()),
                format!("{}/{}", self.endpoint.base_path(), encoded),
            )
        };
        let url = format!("{}://{}{}", self.endpoint.scheme(), host, path);
        (url, host, path)
    }

    fn signed_headers(
        &self,
        host: &str,
        payload_hash: &str,
        extras: &[(String, String)],
        now: DateTime<Utc>,
    ) -> (String, Vec<(String, String)>) {
        let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
        let mut headers = vec![
            ("host".to_owned(), host.to_owned()),
            ("x-amz-content-sha256".to_owned(), payload_hash.to_owned()),
            ("x-amz-date".to_owned(), amz_date.clone()),
        ];
        if let Some(token) = self.config.credentials.session_token() {
            headers.push(("x-amz-security-token".to_owned(), token.expose().to_owned()));
        }
        headers.extend(extras.iter().cloned());
        (amz_date, headers)
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
        let scope = format!("{date}/{}/s3/aws4_request", self.region);
        let string_to_sign = sign::sigv4_string_to_sign(
            &now.format("%Y%m%dT%H%M%SZ").to_string(),
            &scope,
            &canonical,
        );
        let signature = sign::sigv4_signature(
            self.config.credentials.secret().expose(),
            &date,
            &self.region,
            "s3",
            &string_to_sign,
        );
        format!(
            "AWS4-HMAC-SHA256 Credential={}/{scope}, SignedHeaders={signed_headers}, Signature={signature}",
            self.config.credentials.access_key()
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
                format!("x-amz-meta-{}", name.to_ascii_lowercase()),
                value.clone(),
            ));
        }
        extras
    }

    fn request(
        &self,
        method: Method,
        key: &ObjectKey,
        body: Bytes,
        headers: Vec<(String, String)>,
    ) -> rdd_core::Result<HttpRequest> {
        let (url, _, _) = self.object_location(key);
        let mut request = HttpRequest::new(method, url).body(body);
        for (name, value) in headers {
            let (name, value) = http_client::header_pair(&name, &value, "cabecera firmada")?;
            request = request.header(name, value);
        }
        Ok(request)
    }
}

impl ObjectStorage for S3Storage {
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
            let (_, host, path) = self.object_location(&key);
            let extras = self.extras(&options);
            let (_, mut headers) = self.signed_headers(&host, &payload_hash, &extras, now);
            let authorization = self.authorization("PUT", &path, &headers, &payload_hash, now);
            headers.push(("authorization".to_owned(), authorization));
            let size = body.len() as u64;
            let request = self.request(Method::PUT, &key, body, headers)?;
            let response = http_client::send(&self.client, request, self.max_bytes).await?;
            map_put(&key, &options, size, response)
        })
    }

    fn get(&self, key: &ObjectKey) -> BoxFuture<'_, rdd_core::Result<Option<StoredObject>>> {
        let key = key.clone();
        Box::pin(async move {
            let now = Utc::now();
            let payload_hash = sign::sha256_empty_hex();
            let (_, host, path) = self.object_location(&key);
            let (_, mut headers) = self.signed_headers(&host, &payload_hash, &[], now);
            let authorization = self.authorization("GET", &path, &headers, &payload_hash, now);
            headers.push(("authorization".to_owned(), authorization));
            let request = self.request(Method::GET, &key, Bytes::new(), headers)?;
            let response = http_client::send(&self.client, request, self.max_bytes).await?;
            map_get(&key, response)
        })
    }

    fn head(&self, key: &ObjectKey) -> BoxFuture<'_, rdd_core::Result<Option<ObjectMeta>>> {
        let key = key.clone();
        Box::pin(async move {
            let now = Utc::now();
            let payload_hash = sign::sha256_empty_hex();
            let (_, host, path) = self.object_location(&key);
            let (_, mut headers) = self.signed_headers(&host, &payload_hash, &[], now);
            let authorization = self.authorization("HEAD", &path, &headers, &payload_hash, now);
            headers.push(("authorization".to_owned(), authorization));
            let request = self.request(Method::HEAD, &key, Bytes::new(), headers)?;
            let response = http_client::send(&self.client, request, 64 * 1024).await?;
            map_head(&key, response)
        })
    }

    fn delete(&self, key: &ObjectKey) -> BoxFuture<'_, rdd_core::Result<()>> {
        let key = key.clone();
        Box::pin(async move {
            let now = Utc::now();
            let payload_hash = sign::sha256_empty_hex();
            let (_, host, path) = self.object_location(&key);
            let (_, mut headers) = self.signed_headers(&host, &payload_hash, &[], now);
            let authorization = self.authorization("DELETE", &path, &headers, &payload_hash, now);
            headers.push(("authorization".to_owned(), authorization));
            let request = self.request(Method::DELETE, &key, Bytes::new(), headers)?;
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
        return Err(http_client::provider_error("S3", &response));
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
        return Err(http_client::provider_error("S3", &response));
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
        return Err(http_client::provider_error("S3", &response));
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
    Err(http_client::provider_error("S3", &response))
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
        "el objeto mide {size} bytes y el límite es {max} (RF-272)"
    ))
}

fn validate_bucket(bucket: &str) -> Result<(), UploadConfigError> {
    let valid = (3..=63).contains(&bucket.len())
        && bucket.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'.'
        });
    if valid {
        Ok(())
    } else {
        Err(UploadConfigError::Config(format!(
            "el bucket S3 `{bucket}` no es válido: 3 a 63 caracteres en minúscula, dígitos, `-` o `.`"
        )))
    }
}

fn validate_region(region: &str) -> Result<(), UploadConfigError> {
    let valid = !region.is_empty()
        && region
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-');
    if valid {
        Ok(())
    } else {
        Err(UploadConfigError::Config(format!(
            "la región S3 `{region}` no es válida"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::SystemTime;

    fn storage(endpoint: Option<&str>) -> S3Storage {
        let mut config = S3Config::new(
            "mi-bucket",
            Credentials::new("AKIAIOSFODNN7EXAMPLE", "wJalrXUtnFEMI/K7MDENG"),
        );
        if let Some(endpoint) = endpoint {
            config = config.endpoint(endpoint);
        }
        S3Storage::new(config).unwrap()
    }

    fn key() -> ObjectKey {
        ObjectKey::parse("files/a/uno.txt").unwrap()
    }

    #[test]
    fn el_endpoint_activa_path_style_y_sin_el_virtual_host() {
        let local = storage(Some("http://127.0.0.1:9000"));
        let (url, host, path) = local.object_location(&key());
        assert_eq!(host, "127.0.0.1:9000");
        assert_eq!(path, "/mi-bucket/files/a/uno.txt");
        assert_eq!(url, "http://127.0.0.1:9000/mi-bucket/files/a/uno.txt");

        let aws = storage(None);
        let (url, host, path) = aws.object_location(&key());
        assert_eq!(host, "mi-bucket.s3.us-east-1.amazonaws.com");
        assert_eq!(path, "/files/a/uno.txt");
        assert_eq!(
            url,
            "https://mi-bucket.s3.us-east-1.amazonaws.com/files/a/uno.txt"
        );
    }

    #[test]
    fn la_url_codifica_por_segmento() {
        let local = storage(Some("http://127.0.0.1:9000"));
        let (url, _, _) =
            local.object_location(&ObjectKey::parse("mis archivos/año 1.txt").unwrap());
        assert!(
            url.ends_with("/mi-bucket/mis%20archivos/a%C3%B1o%201.txt"),
            "{url}"
        );
    }

    #[test]
    fn el_token_de_sesion_entra_en_las_cabeceras_firmadas() {
        let config = S3Config::new(
            "mi-bucket",
            Credentials::new("AKIA", "secreto").with_session_token("token-temporal"),
        );
        let storage = S3Storage::new(config).unwrap();
        let (_, headers) = storage.signed_headers(
            "mi-bucket.s3.us-east-1.amazonaws.com",
            &sign::sha256_empty_hex(),
            &[],
            Utc::now(),
        );
        assert!(
            headers
                .iter()
                .any(|(name, value)| name == "x-amz-security-token" && value == "token-temporal")
        );
    }

    #[test]
    fn la_firma_es_determinista_con_fecha_fija() {
        let storage = storage(Some("http://127.0.0.1:9000"));
        let now: DateTime<Utc> = "2013-05-24T00:00:00Z".parse().unwrap();
        let payload_hash = sign::sha256_hex(b"hola");
        let (_, headers) = storage.signed_headers("127.0.0.1:9000", &payload_hash, &[], now);
        let first = storage.authorization(
            "PUT",
            "/mi-bucket/files/a/uno.txt",
            &headers,
            &payload_hash,
            now,
        );
        let second = storage.authorization(
            "PUT",
            "/mi-bucket/files/a/uno.txt",
            &headers,
            &payload_hash,
            now,
        );
        assert_eq!(first, second);
        assert!(
            first.contains("Credential=AKIAIOSFODNN7EXAMPLE/20130524/us-east-1/s3/aws4_request")
        );
        assert!(first.contains("SignedHeaders=host;x-amz-content-sha256;x-amz-date"));
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
    fn la_metadata_sale_de_las_cabeceras() {
        let mut headers = HeaderMap::new();
        headers.insert(header::CONTENT_TYPE, "text/plain".parse().unwrap());
        headers.insert(header::ETAG, "\"abc\"".parse().unwrap());
        headers.insert(
            header::LAST_MODIFIED,
            "Tue, 15 Nov 1994 12:45:26 GMT".parse().unwrap(),
        );
        let meta = meta_from_headers(&key(), &headers, 7);
        assert_eq!(meta.size, 7);
        assert_eq!(meta.content_type.as_deref(), Some("text/plain"));
        assert_eq!(meta.etag.as_deref(), Some("\"abc\""));
        assert!(meta.last_modified.is_some());
        assert!(meta.last_modified.unwrap() > SystemTime::UNIX_EPOCH);
    }

    #[test]
    fn from_lookup_prefiere_el_constructor_y_nombra_la_variable_ausente() {
        let env = |name: &str| match name {
            "AWS_S3_BUCKET" => Some("bucket-env".to_owned()),
            "AWS_ACCESS_KEY_ID" => Some("AKIA-ENV".to_owned()),
            "AWS_SECRET_ACCESS_KEY" => Some("secreto-env".to_owned()),
            "AWS_REGION" => Some("eu-west-1".to_owned()),
            "AWS_MAX_OBJECT_BYTES" => Some("1024".to_owned()),
            _ => None,
        };
        let config = S3Config::from_lookup(env).unwrap();
        assert_eq!(config.bucket(), "bucket-env");
        assert_eq!(config.region_value(), "eu-west-1");
        assert_eq!(config.max_bytes(), 1024);
        let overridden = config.clone().region("us-west-2").max_object_bytes(2048);
        assert_eq!(overridden.region_value(), "us-west-2");
        assert_eq!(overridden.max_bytes(), 2048);

        let err = S3Config::from_lookup(|name| {
            if name == "AWS_S3_BUCKET" {
                Some("bucket".to_owned())
            } else {
                None
            }
        })
        .unwrap_err();
        assert!(err.to_string().contains("AWS_ACCESS_KEY_ID"), "{err}");
    }
}
