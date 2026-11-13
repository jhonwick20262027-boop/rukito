//! Adaptador de Azure Blob Storage con `SharedKey` (§10.13, RF-273).
//!
//! La clave de cuenta llega en Base64 y se decodifica una vez al construir el
//! adaptador; la firma canónica sigue la cadena publicada por Azure con
//! `x-ms-date` y `x-ms-version` fijos por petición.

use base64::Engine as _;
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
/// Versión del servicio enviada en `x-ms-version`.
pub const API_VERSION: &str = "2021-12-02";

/// Configuración de Azure Blob, por constructor o desde el entorno.
#[derive(Debug, Clone)]
pub struct AzureBlobConfig {
    account: String,
    access_key: Secret,
    container: String,
    endpoint: Option<String>,
    max_object_bytes: Option<usize>,
}

impl AzureBlobConfig {
    /// Configuración mínima: cuenta, clave de cuenta y contenedor.
    pub fn new(
        account: impl Into<String>,
        access_key: impl Into<String>,
        container: impl Into<String>,
    ) -> Self {
        Self {
            account: account.into(),
            access_key: Secret::new(access_key),
            container: container.into(),
            endpoint: None,
            max_object_bytes: None,
        }
    }

    /// Endpoint alternativo; Azurite usa `http://127.0.0.1:10000/{cuenta}`.
    pub fn endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = Some(endpoint.into());
        self
    }

    /// Límite de objeto aceptado por el adaptador.
    pub fn max_object_bytes(mut self, max: usize) -> Self {
        self.max_object_bytes = Some(max);
        self
    }

    /// Cuenta configurada.
    pub fn account(&self) -> &str {
        &self.account
    }

    /// Contenedor configurado.
    pub fn container(&self) -> &str {
        &self.container
    }

    /// Endpoint configurado, si lo hay.
    pub fn endpoint_value(&self) -> Option<&str> {
        self.endpoint.as_deref()
    }

    /// Límite efectivo.
    pub fn max_bytes(&self) -> usize {
        self.max_object_bytes.unwrap_or(DEFAULT_MAX_OBJECT_BYTES)
    }

    /// Lee `AZURE_STORAGE_*` del entorno del proceso.
    pub fn from_env() -> Result<Self, UploadConfigError> {
        Self::from_lookup(|name| std::env::var(name).ok().filter(|value| !value.is_empty()))
    }

    /// Lee las mismas variables de una fuente inyectable (pruebas sin tocar el
    /// entorno del proceso, que en edición 2024 es `unsafe`).
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, UploadConfigError> {
        let required = |name| lookup(name).ok_or(UploadConfigError::Missing(name));
        let account = required("AZURE_STORAGE_ACCOUNT")?;
        let access_key = required("AZURE_STORAGE_ACCESS_KEY")?;
        let container = required("AZURE_STORAGE_CONTAINER")?;
        let max_object_bytes = match lookup("AZURE_STORAGE_MAX_OBJECT_BYTES") {
            Some(raw) => Some(endpoint::parse_usize(
                "AZURE_STORAGE_MAX_OBJECT_BYTES",
                &raw,
            )?),
            None => None,
        };
        Ok(Self {
            account,
            access_key: Secret::new(access_key),
            container,
            endpoint: lookup("AZURE_STORAGE_ENDPOINT"),
            max_object_bytes,
        })
    }
}

/// Storage de objetos sobre la API REST de Azure Blob.
pub struct AzureBlobStorage {
    config: AzureBlobConfig,
    endpoint: Endpoint,
    key: Vec<u8>,
    max_bytes: usize,
    client: HttpClient,
}

impl AzureBlobStorage {
    /// Valida la configuración, decodifica la clave y prepara el cliente HTTP.
    pub fn new(config: AzureBlobConfig) -> Result<Self, UploadConfigError> {
        validate_account(&config.account)?;
        validate_container(&config.container)?;
        let key = base64::engine::general_purpose::STANDARD
            .decode(config.access_key.expose())
            .map_err(|_| {
                UploadConfigError::Config(
                    "AZURE_STORAGE_ACCESS_KEY no es un valor Base64 válido".to_owned(),
                )
            })?;
        if key.is_empty() {
            return Err(UploadConfigError::Config(
                "la clave de la cuenta de Azure no puede estar vacía".to_owned(),
            ));
        }
        let endpoint = match config.endpoint_value() {
            Some(raw) => Endpoint::parse(raw)?,
            None => Endpoint::parse(&format!("https://{}.blob.core.windows.net", config.account))?,
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
            key,
            max_bytes,
            client,
        })
    }

    /// Construye el adaptador leyendo `AZURE_STORAGE_*` del entorno.
    pub fn from_env() -> Result<Self, UploadConfigError> {
        Self::new(AzureBlobConfig::from_env()?)
    }

    /// Configuración efectiva.
    pub fn config(&self) -> &AzureBlobConfig {
        &self.config
    }

    /// URL, host, ruta y recurso canónico de un objeto.
    fn object_location(&self, key: &ObjectKey) -> (String, String, String, String) {
        let encoded = key.encoded_path();
        let base = self.endpoint.base_path();
        let path = if base.is_empty() {
            format!("/{}/{}", self.config.container, encoded)
        } else {
            format!("{base}/{}/{}", self.config.container, encoded)
        };
        // Azurite repite la cuenta en el recurso canónico; la documentación lo
        // señala como esperado.
        let resource = format!("/{}{}", self.config.account, path);
        (
            self.endpoint.url(&path),
            self.endpoint.host().to_owned(),
            path,
            resource,
        )
    }

    fn signed_headers(
        &self,
        extras: &[(String, String)],
        now: DateTime<Utc>,
    ) -> Vec<(String, String)> {
        let mut headers = vec![
            (
                "x-ms-date".to_owned(),
                now.format("%a, %d %b %Y %H:%M:%S GMT").to_string(),
            ),
            ("x-ms-version".to_owned(), API_VERSION.to_owned()),
        ];
        headers.extend(extras.iter().cloned());
        headers
    }

    fn authorization(
        &self,
        method: &str,
        content_length: &str,
        content_type: &str,
        headers: &[(String, String)],
        resource: &str,
    ) -> String {
        let string_to_sign =
            canonical_string(method, content_length, content_type, headers, resource);
        let signature = shared_key_signature(&self.key, &string_to_sign);
        format!("SharedKey {}:{signature}", self.config.account)
    }

    fn extras(&self, options: &PutOptions) -> Vec<(String, String)> {
        let mut extras = vec![("x-ms-blob-type".to_owned(), "BlockBlob".to_owned())];
        if let Some(value) = &options.cache_control {
            extras.push(("x-ms-blob-cache-control".to_owned(), value.clone()));
        }
        for (name, value) in &options.metadata {
            extras.push((
                format!("x-ms-meta-{}", name.to_ascii_lowercase()),
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

impl ObjectStorage for AzureBlobStorage {
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
            let (url, _, _, resource) = self.object_location(&key);
            let extras = self.extras(&options);
            let mut headers = self.signed_headers(&extras, now);
            let content_type = options.content_type.clone().unwrap_or_default();
            let content_length = if body.is_empty() {
                String::new()
            } else {
                body.len().to_string()
            };
            let authorization =
                self.authorization("PUT", &content_length, &content_type, &headers, &resource);
            headers.push(("authorization".to_owned(), authorization));
            if !content_type.is_empty() {
                headers.push(("content-type".to_owned(), content_type));
            }
            let size = body.len() as u64;
            let request = self.request(Method::PUT, url, body, headers)?;
            let response = http_client::send(&self.client, request, self.max_bytes).await?;
            map_put(&key, &options, size, response)
        })
    }

    fn get(&self, key: &ObjectKey) -> BoxFuture<'_, rdd_core::Result<Option<StoredObject>>> {
        let key = key.clone();
        Box::pin(async move {
            let (url, _, _, resource) = self.object_location(&key);
            let mut headers = self.signed_headers(&[], Utc::now());
            let authorization = self.authorization("GET", "", "", &headers, &resource);
            headers.push(("authorization".to_owned(), authorization));
            let request = self.request(Method::GET, url, Bytes::new(), headers)?;
            let response = http_client::send(&self.client, request, self.max_bytes).await?;
            map_get(&key, response)
        })
    }

    fn head(&self, key: &ObjectKey) -> BoxFuture<'_, rdd_core::Result<Option<ObjectMeta>>> {
        let key = key.clone();
        Box::pin(async move {
            let (url, _, _, resource) = self.object_location(&key);
            let mut headers = self.signed_headers(&[], Utc::now());
            let authorization = self.authorization("HEAD", "", "", &headers, &resource);
            headers.push(("authorization".to_owned(), authorization));
            let request = self.request(Method::HEAD, url, Bytes::new(), headers)?;
            let response = http_client::send(&self.client, request, 64 * 1024).await?;
            map_head(&key, response)
        })
    }

    fn delete(&self, key: &ObjectKey) -> BoxFuture<'_, rdd_core::Result<()>> {
        let key = key.clone();
        Box::pin(async move {
            let (url, _, _, resource) = self.object_location(&key);
            let mut headers = self.signed_headers(&[], Utc::now());
            let authorization = self.authorization("DELETE", "", "", &headers, &resource);
            headers.push(("authorization".to_owned(), authorization));
            let request = self.request(Method::DELETE, url, Bytes::new(), headers)?;
            let response = http_client::send(&self.client, request, 64 * 1024).await?;
            map_delete(response)
        })
    }
}

/// Cadena canónica de `SharedKey` para Blob (documentación de Azure).
fn canonical_string(
    method: &str,
    content_length: &str,
    content_type: &str,
    headers: &[(String, String)],
    resource: &str,
) -> String {
    let mut canonical_headers: Vec<(String, String)> = headers
        .iter()
        .filter(|(name, _)| name.to_ascii_lowercase().starts_with("x-ms-"))
        .map(|(name, value)| (name.to_ascii_lowercase(), value.trim().to_owned()))
        .collect();
    canonical_headers.sort_by(|left, right| left.0.cmp(&right.0));
    let mut canonical = String::new();
    for (name, value) in canonical_headers {
        canonical.push_str(&name);
        canonical.push(':');
        canonical.push_str(&value);
        canonical.push('\n');
    }
    let fields = [
        method,
        "",
        "",
        content_length,
        "",
        content_type,
        "",
        "",
        "",
        "",
        "",
        "",
    ];
    format!("{}\n{canonical}{resource}", fields.join("\n"))
}

/// HMAC-SHA256 con la clave decodificada, en Base64 estándar.
fn shared_key_signature(key: &[u8], string_to_sign: &str) -> String {
    base64::engine::general_purpose::STANDARD
        .encode(sign::hmac_sha256(key, string_to_sign.as_bytes()))
}

fn map_put(
    key: &ObjectKey,
    options: &PutOptions,
    size: u64,
    response: HttpResponse,
) -> rdd_core::Result<ObjectMeta> {
    if !response.is_success() {
        return Err(http_client::provider_error("Azure Blob", &response));
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
        return Err(http_client::provider_error("Azure Blob", &response));
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
        return Err(http_client::provider_error("Azure Blob", &response));
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
    Err(http_client::provider_error("Azure Blob", &response))
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
        "el objeto mide {size} bytes y el límite es {max} (RF-273)"
    ))
}

fn validate_account(account: &str) -> Result<(), UploadConfigError> {
    let valid = (3..=24).contains(&account.len())
        && account
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit());
    if valid {
        Ok(())
    } else {
        Err(UploadConfigError::Config(format!(
            "la cuenta de Azure `{account}` no es válida: 3 a 24 caracteres en minúscula o dígitos"
        )))
    }
}

fn validate_container(container: &str) -> Result<(), UploadConfigError> {
    let valid = (3..=63).contains(&container.len())
        && container
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && container
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_alphanumeric())
        && container
            .bytes()
            .last()
            .is_some_and(|byte| byte.is_ascii_alphanumeric());
    if valid {
        Ok(())
    } else {
        Err(UploadConfigError::Config(format!(
            "el contenedor de Azure `{container}` no es válido: 3 a 63 caracteres en minúscula, \
             dígitos o `-`, empezando y terminando en letra o dígito"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AZURITE_KEY: &str =
        "Eby8vdM02xNOcqFlqUwJPLlmEtlCDXJ1OUzFT50uSRZ6IFsuFq2UVErCz4I6tq/K1SZFPTOtr/KBHBeksoGMGw==";

    fn storage(endpoint: Option<&str>) -> AzureBlobStorage {
        let mut config = AzureBlobConfig::new("devstoreaccount1", AZURITE_KEY, "mi-contenedor");
        if let Some(endpoint) = endpoint {
            config = config.endpoint(endpoint);
        }
        AzureBlobStorage::new(config).unwrap()
    }

    fn key() -> ObjectKey {
        ObjectKey::parse("files/a/uno.txt").unwrap()
    }

    #[test]
    fn el_endpoint_por_defecto_y_azurite_generan_la_ruta_esperada() {
        let azure = storage(None);
        let (url, host, path, resource) = azure.object_location(&key());
        assert_eq!(host, "devstoreaccount1.blob.core.windows.net");
        assert_eq!(path, "/mi-contenedor/files/a/uno.txt");
        assert_eq!(
            url,
            "https://devstoreaccount1.blob.core.windows.net/mi-contenedor/files/a/uno.txt"
        );
        assert_eq!(resource, "/devstoreaccount1/mi-contenedor/files/a/uno.txt");

        let azurite = storage(Some("http://127.0.0.1:10000/devstoreaccount1"));
        let (url, host, path, resource) = azurite.object_location(&key());
        assert_eq!(host, "127.0.0.1:10000");
        assert_eq!(path, "/devstoreaccount1/mi-contenedor/files/a/uno.txt");
        assert_eq!(
            url,
            "http://127.0.0.1:10000/devstoreaccount1/mi-contenedor/files/a/uno.txt"
        );
        // Azurite repite la cuenta en el recurso canónico.
        assert_eq!(
            resource,
            "/devstoreaccount1/devstoreaccount1/mi-contenedor/files/a/uno.txt"
        );
    }

    #[test]
    fn el_vector_de_shared_key_reproduce_la_firma() {
        // Cadena canónica de la documentación de Azure; la firma esperada se
        // calculó con una implementación HMAC independiente (OpenSSL/Node).
        let storage = storage(Some("http://127.0.0.1:10000/devstoreaccount1"));
        let mut headers = vec![
            ("x-ms-blob-type".to_owned(), "BlockBlob".to_owned()),
            (
                "x-ms-date".to_owned(),
                "Fri, 13 Nov 2026 00:00:00 GMT".to_owned(),
            ),
            ("x-ms-version".to_owned(), API_VERSION.to_owned()),
        ];
        headers.reverse();
        let string_to_sign = canonical_string(
            "PUT",
            "5",
            "text/plain",
            &headers,
            "/devstoreaccount1/mi-contenedor/files/a/uno.txt",
        );
        assert_eq!(
            string_to_sign,
            "PUT\n\n\n5\n\ntext/plain\n\n\n\n\n\n\n\
             x-ms-blob-type:BlockBlob\n\
             x-ms-date:Fri, 13 Nov 2026 00:00:00 GMT\n\
             x-ms-version:2021-12-02\n\
             /devstoreaccount1/mi-contenedor/files/a/uno.txt"
        );
        assert_eq!(
            shared_key_signature(&storage.key, &string_to_sign),
            "9jX8/oxIQ700acdd/kOKrMvFPnsRJY1EMqG1FFa52G8="
        );
    }

    #[test]
    fn la_autorizacion_nombra_la_cuenta_y_la_fecha_es_del_formato_rfc1123() {
        let storage = storage(None);
        let now: DateTime<Utc> = "2026-11-13T00:00:00Z".parse().unwrap();
        let headers = storage.signed_headers(&[], now);
        assert_eq!(headers[0].0, "x-ms-date");
        assert_eq!(headers[0].1, "Fri, 13 Nov 2026 00:00:00 GMT");
        assert_eq!(
            headers[1],
            ("x-ms-version".to_owned(), API_VERSION.to_owned())
        );
        let authorization = storage.authorization(
            "GET",
            "",
            "",
            &headers,
            "/devstoreaccount1/mi-contenedor/files/a/uno.txt",
        );
        assert!(authorization.starts_with("SharedKey devstoreaccount1:"));
    }

    #[test]
    fn put_firma_blockblob_y_los_metadatos() {
        let storage = storage(None);
        let options = PutOptions::new()
            .content_type("text/plain")
            .cache_control("max-age=60")
            .metadata("origen", "demo");
        let extras = storage.extras(&options);
        assert!(
            extras
                .iter()
                .any(|(name, value)| name == "x-ms-blob-type" && value == "BlockBlob")
        );
        assert!(
            extras
                .iter()
                .any(|(name, value)| name == "x-ms-blob-cache-control" && value == "max-age=60")
        );
        assert!(
            extras
                .iter()
                .any(|(name, value)| name == "x-ms-meta-origen" && value == "demo")
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
        assert!(map_delete(not_found(http::StatusCode::ACCEPTED)).is_ok());
    }

    #[test]
    fn from_lookup_prefiere_el_constructor_y_nombra_la_variable_ausente() {
        let env = |name: &str| match name {
            "AZURE_STORAGE_ACCOUNT" => Some("cuentaenv".to_owned()),
            "AZURE_STORAGE_ACCESS_KEY" => Some(AZURITE_KEY.to_owned()),
            "AZURE_STORAGE_CONTAINER" => Some("contenedor-env".to_owned()),
            "AZURE_STORAGE_ENDPOINT" => Some("http://127.0.0.1:10000/cuentaenv".to_owned()),
            "AZURE_STORAGE_MAX_OBJECT_BYTES" => Some("1024".to_owned()),
            _ => None,
        };
        let config = AzureBlobConfig::from_lookup(env).unwrap();
        assert_eq!(config.account(), "cuentaenv");
        assert_eq!(config.container(), "contenedor-env");
        assert_eq!(config.max_bytes(), 1024);
        let overridden = config.clone().max_object_bytes(2048);
        assert_eq!(overridden.max_bytes(), 2048);

        let err = AzureBlobConfig::from_lookup(|name| {
            if name == "AZURE_STORAGE_ACCOUNT" {
                Some("cuenta".to_owned())
            } else {
                None
            }
        })
        .unwrap_err();
        assert!(
            err.to_string().contains("AZURE_STORAGE_ACCESS_KEY"),
            "{err}"
        );
    }

    #[test]
    fn rechaza_una_clave_que_no_es_base64_sin_mostrarla() {
        let config = AzureBlobConfig::new("devstoreaccount1", "no-es-base64-ñ", "mi-contenedor");
        let err = match AzureBlobStorage::new(config) {
            Ok(_) => panic!("aceptó una clave que no es Base64"),
            Err(err) => err,
        };
        let message = err.to_string();
        assert!(message.contains("Base64"), "{message}");
        assert!(!message.contains("no-es-base64"), "{message}");
    }
}
