//! Montaje de `/api-docs` sobre el `HttpApp` (§10.10, RP-032).
//!
//! La plataforma no interpreta OpenAPI: lee el cuerpo con el límite de la app
//! (RP-006), ejecuta la cadena global con `HttpApp::prepare` (RP-028), llama a
//! [`OpenApiHttp::handle`] y traduce la respuesta. La UI y el JSON ya salen del
//! crate neutral con sus cabeceras; aquí solo se vuelcan al router.

use std::sync::Arc;

use axum::body::Body;
use axum::extract::Request;
use axum::http::header::{CACHE_CONTROL, CONTENT_LENGTH};
use axum::response::Response;
use rdd_core::http::{HttpApp, Prepared, normalize_path};
use rdd_openapi::{OpenApiHttp, OpenApiHttpResponse};

use crate::shim::{ReadBody, internal, normalize_uri, payload_too_large, read_capped};

/// Atiende una petición dirigida a una ruta de la documentación.
pub(crate) async fn serve_openapi(
    app: Arc<HttpApp>,
    openapi: Arc<OpenApiHttp>,
    req: Request,
) -> Response {
    let limit = app.limits.body_limit_bytes;
    let declared_too_big = req
        .headers()
        .get(CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<usize>().ok())
        .is_some_and(|len| len > limit);

    let (mut parts, body) = req.into_parts();
    let path = normalize_path(parts.uri.path());
    if declared_too_big {
        return no_store(payload_too_large(limit, &path));
    }
    let bytes = match read_capped(body, limit).await {
        Ok(bytes) => bytes,
        Err(ReadBody::TooLarge) => return no_store(payload_too_large(limit, &path)),
        Err(ReadBody::Failed(err)) => return no_store(internal(&err)),
    };
    parts.uri = match normalize_uri(&parts.uri) {
        Ok(uri) => uri,
        Err(err) => return no_store(internal(&err)),
    };

    let prepared = app.prepare(http::Request::from_parts(parts, bytes)).await;
    let req = match prepared {
        Prepared::Reject(resp) => return no_store(into_axum(resp)),
        Prepared::Continue(req) => req,
    };

    no_store(match openapi.handle(req).await {
        OpenApiHttpResponse::Html(resp) | OpenApiHttpResponse::Json(resp) => into_axum(resp),
    })
}

/// RF-251: toda respuesta de `/api-docs` es `no-store`.
fn no_store(mut resp: Response) -> Response {
    resp.headers_mut().insert(
        CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-store"),
    );
    resp
}

/// Respuesta del núcleo al tipo de Axum.
fn into_axum(resp: rdd_core::http::Response) -> Response {
    let (parts, body) = resp.into_parts();
    Response::from_parts(parts, Body::from(body))
}
