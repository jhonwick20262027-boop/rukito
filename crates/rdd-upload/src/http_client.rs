//! Cliente HTTP compartido por los adaptadores (§10.13, V29).
//!
//! Un solo cliente `hyper` + `hyper-rustls` con el proveedor `ring` y raíces
//! webpki. Nunca se usa un servidor, Tower ni un SDK de nube (RA-025).

use anyhow::anyhow;
use bytes::Bytes;
use http::header::{HeaderMap, HeaderName, HeaderValue};
use http::{Method, Request, StatusCode};
use http_body_util::{BodyExt, Full};

/// Cliente HTTP/1.1 con TLS para `https` y conexión directa para `http`.
pub(crate) type HttpClient = hyper_util::client::legacy::Client<
    hyper_rustls::HttpsConnector<hyper_util::client::legacy::connect::HttpConnector>,
    Full<Bytes>,
>;

/// Construye el cliente una vez por adaptador.
pub(crate) fn client() -> rdd_core::Result<HttpClient> {
    let tls = hyper_rustls::HttpsConnectorBuilder::new()
        .with_provider_and_webpki_roots(rustls::crypto::ring::default_provider())
        .map_err(|err| {
            rdd_core::Error::internal(anyhow!("no se pudo preparar el cliente TLS: {err}"))
        })?
        .https_or_http()
        .enable_http1()
        .build();
    Ok(
        hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
            .build(tls),
    )
}

/// Petición ya firmada, lista para enviar.
pub(crate) struct HttpRequest {
    pub(crate) method: Method,
    pub(crate) url: String,
    pub(crate) headers: Vec<(HeaderName, HeaderValue)>,
    pub(crate) body: Bytes,
}

impl HttpRequest {
    /// Petición sin cabeceras propias.
    pub(crate) fn new(method: Method, url: impl Into<String>) -> Self {
        Self {
            method,
            url: url.into(),
            headers: Vec::new(),
            body: Bytes::new(),
        }
    }

    /// Añade una cabecera con un valor ya validado.
    pub(crate) fn header(mut self, name: HeaderName, value: HeaderValue) -> Self {
        self.headers.push((name, value));
        self
    }

    /// Fija el cuerpo y su longitud.
    pub(crate) fn body(mut self, body: Bytes) -> Self {
        self.body = body;
        self
    }
}

/// Respuesta completa, con el cuerpo ya leído.
pub(crate) struct HttpResponse {
    pub(crate) status: StatusCode,
    pub(crate) headers: HeaderMap,
    pub(crate) body: Bytes,
}

impl HttpResponse {
    /// 2xx.
    pub(crate) fn is_success(&self) -> bool {
        self.status.is_success()
    }

    /// 404, que el puerto traduce a `None`.
    pub(crate) fn is_not_found(&self) -> bool {
        self.status == StatusCode::NOT_FOUND
    }

    /// Cabecera como texto.
    pub(crate) fn header_str(&self, name: &str) -> Option<&str> {
        self.headers.get(name).and_then(|value| value.to_str().ok())
    }
}

/// Convierte un par de texto en cabecera HTTP, rechazando valores no válidos.
pub(crate) fn header_pair(
    name: &str,
    value: &str,
    field: &str,
) -> rdd_core::Result<(HeaderName, HeaderValue)> {
    let name = HeaderName::from_bytes(name.as_bytes()).map_err(|_| {
        rdd_core::Error::invalid(format!("`{field}` no es un nombre de cabecera válido"))
    })?;
    let value = HeaderValue::from_str(value).map_err(|_| {
        rdd_core::Error::invalid(format!("`{field}` contiene caracteres no válidos en HTTP"))
    })?;
    Ok((name, value))
}

/// Convierte `Last-Modified` (IMF-fixdate/RFC 2822) en un instante.
pub(crate) fn parse_http_date(value: &str) -> Option<std::time::SystemTime> {
    chrono::DateTime::parse_from_rfc2822(value)
        .ok()
        .map(|date| date.with_timezone(&chrono::Utc).into())
}

/// Envía la petición y lee el cuerpo, sin reintentos (RF-275).
pub(crate) async fn send(
    client: &HttpClient,
    request: HttpRequest,
    max_body: usize,
) -> rdd_core::Result<HttpResponse> {
    let mut builder = Request::builder().method(request.method).uri(&request.url);
    for (name, value) in &request.headers {
        builder = builder.header(name, value);
    }
    let request = builder
        .body(Full::new(request.body))
        .map_err(|err| rdd_core::Error::internal(anyhow!("petición HTTP inválida: {err}")))?;

    let response = client
        .request(request)
        .await
        .map_err(|err| rdd_core::Error::internal(anyhow!("el proveedor no respondió: {err}")))?;
    let status = response.status();
    let headers = response.headers().clone();
    let body = response
        .into_body()
        .collect()
        .await
        .map_err(|err| rdd_core::Error::internal(anyhow!("respuesta incompleta: {err}")))?
        .to_bytes();
    if body.len() > max_body {
        return Err(rdd_core::Error::internal(anyhow!(
            "la respuesta del proveedor supera el límite de {max_body} bytes"
        )));
    }
    Ok(HttpResponse {
        status,
        headers,
        body,
    })
}

/// Error interno con estado y código del proveedor, sin secretos (RF-275).
pub(crate) fn provider_error(provider: &str, response: &HttpResponse) -> rdd_core::Error {
    let code = provider_code(&response.body).unwrap_or_else(|| "sin código".to_owned());
    rdd_core::Error::internal(anyhow!(
        "{provider} respondió {} ({code})",
        response.status.as_u16()
    ))
}

/// Extrae `<Code>…</Code>` del XML de error de S3, Azure o GCS.
fn provider_code(body: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(body).ok()?;
    let start = text.find("<Code>")? + "<Code>".len();
    let end = text[start..].find("</Code>")? + start;
    let code = text[start..end].trim();
    if code.is_empty() {
        return None;
    }
    Some(code.chars().take(80).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lee_el_codigo_del_xml_de_error() {
        let error = HttpResponse {
            status: StatusCode::FORBIDDEN,
            headers: HeaderMap::new(),
            body: Bytes::from_static(
                b"<Error><Code>SignatureDoesNotMatch</Code><Message>no</Message></Error>",
            ),
        };
        let message = provider_error("S3", &error).to_string();
        assert!(message.contains("403"));
        assert!(message.contains("SignatureDoesNotMatch"));
    }
}
