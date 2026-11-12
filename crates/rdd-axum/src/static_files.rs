//! Montaje de archivos estáticos sobre el `HttpApp` (§10.11, RP-033).
//!
//! La plataforma no interpreta la ruta: lee el cuerpo con el límite de la app
//! (RP-006), ejecuta la cadena global con `HttpApp::prepare` (RP-028), llama a
//! [`StaticFiles::handle`] y traduce la respuesta. Si no hay archivo ni índice
//! de SPA, responde el 404 canónico del núcleo sin repetir la cadena global.

use std::convert::Infallible;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::Request;
use axum::http::header::CONTENT_LENGTH;
use axum::response::Response;
use rdd_core::http::{HttpApp, Prepared, normalize_path};
use rdd_static::StaticFiles;

use crate::shim::{ReadBody, internal, normalize_uri, payload_too_large, read_capped};

/// Atiende una petición dirigida a un montaje estático.
pub(crate) async fn serve_static(
    app: Arc<HttpApp>,
    files: Arc<StaticFiles>,
    req: Request,
) -> Result<Response, Infallible> {
    let limit = app.limits.body_limit_bytes;
    let path = normalize_path(req.uri().path());
    let declared_too_big = req
        .headers()
        .get(CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<usize>().ok())
        .is_some_and(|len| len > limit);

    let (mut parts, body) = req.into_parts();
    if declared_too_big {
        return Ok(payload_too_large(limit, &path));
    }
    let bytes = match read_capped(body, limit).await {
        Ok(bytes) => bytes,
        Err(ReadBody::TooLarge) => return Ok(payload_too_large(limit, &path)),
        Err(ReadBody::Failed(err)) => return Ok(internal(&err)),
    };
    parts.uri = match normalize_uri(&parts.uri) {
        Ok(uri) => uri,
        Err(err) => return Ok(internal(&err)),
    };

    let prepared = app.prepare(http::Request::from_parts(parts, bytes)).await;
    let req = match prepared {
        Prepared::Reject(resp) => return Ok(into_axum(resp)),
        Prepared::Continue(req) => req,
    };

    match files.handle(&req).await {
        Some(resp) => Ok(into_axum(resp)),
        // La cadena global ya corrió en `prepare`; repetirla aquí contaría dos
        // veces rate limits y autorizaciones. Se devuelve el 404 canónico del
        // núcleo, idéntico al del `fallback` de la app.
        None => {
            let err = rdd_core::Error::not_found(format!("la ruta {path} no existe"));
            Ok(into_axum(rdd_core::http::response::error_response_for(
                &err, &path,
            )))
        }
    }
}

/// Respuesta del núcleo al tipo de Axum.
fn into_axum(resp: rdd_core::http::Response) -> Response {
    let (parts, body) = resp.into_parts();
    Response::from_parts(parts, Body::from(body))
}
