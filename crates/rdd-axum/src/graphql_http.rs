//! Montaje de `/graphql` sobre el `HttpApp` (§10.9, RP-030).
//!
//! La plataforma no interpreta GraphQL: lee el cuerpo con el límite de la app
//! (RP-006), ejecuta la cadena global con `HttpApp::prepare` (RP-028), llama a
//! [`GraphqlHttp::handle`] y traduce la respuesta. El SSE solo se vuelca: la
//! cola de `Bytes` pasa a ser el cuerpo con `Body::from_stream` (RP-031).

use std::convert::Infallible;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::Request;
use axum::http::StatusCode;
use axum::http::header::{CACHE_CONTROL, CONTENT_LENGTH, CONTENT_TYPE};
use axum::response::Response;
use bytes::Bytes;
use rdd_core::http::{HttpApp, Prepared, normalize_path};
use rdd_graphql::{GraphqlHttp, GraphqlHttpResponse};

use crate::shim::{ReadBody, internal, normalize_uri, payload_too_large, read_capped};

/// Atiende una petición dirigida a la ruta del montaje.
pub(crate) async fn serve_graphql(
    app: Arc<HttpApp>,
    graphql: Arc<GraphqlHttp>,
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

    no_store(match graphql.handle(req).await {
        GraphqlHttpResponse::Json(resp) | GraphqlHttpResponse::Html(resp) => into_axum(resp),
        GraphqlHttpResponse::Sse(stream) => sse(stream),
    })
}

/// RF-238: el cuerpo se bombea tal cual y las cabeceras son las del flujo.
fn sse(stream: rdd_graphql::SseStream) -> Response {
    let rx = stream.into_receiver();
    let events = futures::stream::unfold(rx, |mut rx| async move {
        rx.recv()
            .await
            .map(|bytes| (Ok::<Bytes, Infallible>(bytes), rx))
    });
    let mut resp = Response::new(Body::from_stream(events));
    *resp.status_mut() = StatusCode::OK;
    resp.headers_mut().insert(
        CONTENT_TYPE,
        axum::http::HeaderValue::from_static("text/event-stream"),
    );
    resp
}

/// RF-234: toda respuesta de `/graphql` es `no-store`.
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
