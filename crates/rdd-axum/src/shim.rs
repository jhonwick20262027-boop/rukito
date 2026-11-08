//! El puente entre una petición de Axum y una `RouteDef` ya compuesta (§4.4).
//!
//! La plataforma no decide nada: captura parámetros, normaliza la barra final,
//! aplica el límite de cuerpo y entrega la petición al handler del núcleo, que ya
//! trae su cadena completa y sus errores materializados (RP-004, RP-005).

use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::Request;
use axum::http::{Method, StatusCode, Uri, header::CONTENT_LENGTH};
use axum::response::Response;
use bytes::Bytes;
use futures::StreamExt;
use rdd_core::error::{Error, Problem};
use rdd_core::http::{
    AllowedMethods, BoxHandler, Limits, invoke_guarded, normalize_path, problem_response,
    with_extensions,
};

/// Las rutas del núcleo que comparten ruta declarada, indexadas por método.
///
/// Axum enruta por path, pero el 405 lo decide el núcleo (RP-007) para que el
/// cuerpo y la cabecera `Allow` sean idénticos a los de `dispatch`; por eso cada
/// path se registra con `any(...)` y este mapa resuelve el verbo.
pub(crate) struct RouteEntry {
    /// Método → handler ya cerrado.
    methods: HashMap<Method, BoxHandler>,
    /// Verbos registrados en la ruta, para `Allow`.
    allowed: Vec<Method>,
    /// `HttpApp::fallback`, también envuelto por los globales (RF-081).
    fallback: BoxHandler,
    /// Límites de la app (RP-006).
    limits: Limits,
}

impl RouteEntry {
    pub(crate) fn new(
        methods: HashMap<Method, BoxHandler>,
        fallback: BoxHandler,
        limits: Limits,
    ) -> Self {
        let mut allowed: Vec<Method> = methods.keys().cloned().collect();
        allowed.sort_by_key(|method| method.as_str().to_owned());
        Self {
            methods,
            allowed,
            fallback,
            limits,
        }
    }

    /// Verbo registrado en esta ruta, si lo hay.
    fn handler_for(&self, method: &Method) -> Option<&BoxHandler> {
        self.methods.get(method)
    }
}

/// Por qué no se pudo produce un cuerpo utilizable.
enum ReadBody {
    /// RP-006: supera `limits.body_limit_bytes`; se responde 413 sin leer más.
    TooLarge,
    /// El cliente cortó la conexión o el cuerpo vino corrupto.
    Failed(Error),
}

/// Lee el cuerpo en memoria con el límite de la app (RP-002, RP-006).
///
/// Se abandona el stream en cuanto se pasa del límite: el resto del cuerpo nunca
/// se lee.
async fn read_capped(body: Body, limit: usize) -> Result<Bytes, ReadBody> {
    let mut stream = body.into_data_stream();
    let mut collected: Vec<u8> = Vec::new();
    while let Some(chunk) = stream.next().await {
        match chunk {
            Ok(chunk) => {
                if collected.len() + chunk.len() > limit {
                    return Err(ReadBody::TooLarge);
                }
                collected.extend_from_slice(&chunk);
            }
            Err(err) => {
                return Err(ReadBody::Failed(Error::internal(anyhow::anyhow!(
                    "no se pudo leer el cuerpo de la petición: {err}"
                ))));
            }
        }
    }
    Ok(Bytes::from(collected))
}

/// Respuesta Problem Details ya en tipo de Axum (§10.3).
fn problem(status: StatusCode, problem: &Problem) -> Response {
    let resp = problem_response(status, problem, None);
    let (parts, body) = resp.into_parts();
    Response::from_parts(parts, Body::from(body))
}

fn payload_too_large(limit: usize, path: &str) -> Response {
    problem(
        StatusCode::PAYLOAD_TOO_LARGE,
        &Problem::payload_too_large(limit, path),
    )
}

fn internal(err: &Error) -> Response {
    let resp = problem_response(
        StatusCode::INTERNAL_SERVER_ERROR,
        &Problem::from_error(err, "/"),
        None,
    );
    let (parts, body) = resp.into_parts();
    Response::from_parts(parts, Body::from(body))
}

/// `/tasks/` → `/tasks` en el URI de la petición, conservando la query (RP-008).
fn normalize_uri(uri: &Uri) -> Result<Uri, Error> {
    let path = normalize_path(uri.path());
    if path == uri.path() {
        return Ok(uri.clone());
    }
    let rebuilt = match uri.query() {
        Some(query) => format!("{path}?{query}"),
        None => path,
    };
    rebuilt
        .parse::<Uri>()
        .map_err(|err| Error::internal(anyhow::anyhow!("la ruta solicitada no es válida: {err}")))
}

/// Handler de una ruta cuyo path coincide (RP-003, RP-006, RP-007).
pub(crate) async fn serve_route(
    entry: Arc<RouteEntry>,
    params: HashMap<String, String>,
    req: Request,
) -> Result<Response, Infallible> {
    Ok(route_inner(entry, params, req).await)
}

async fn route_inner(
    entry: Arc<RouteEntry>,
    params: HashMap<String, String>,
    req: Request,
) -> Response {
    let method = req.method().clone();
    let limit = entry.limits.body_limit_bytes;

    // RP-006: con `Content-Length` se descarta un cuerpo gigante sin leerlo.
    let declared_too_big = req
        .headers()
        .get(CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<usize>().ok())
        .is_some_and(|len| len > limit);
    let (mut parts, body) = req.into_parts();
    let bytes = if declared_too_big {
        // No se toca el cuerpo: se responde y se cierra la petición.
        return payload_too_large(limit, &normalize_path(parts.uri.path()));
    } else {
        match read_capped(body, limit).await {
            Ok(bytes) => bytes,
            Err(ReadBody::TooLarge) => {
                return payload_too_large(limit, &normalize_path(parts.uri.path()));
            }
            Err(ReadBody::Failed(err)) => return internal(&err),
        }
    };

    parts.uri = match normalize_uri(&parts.uri) {
        Ok(uri) => uri,
        Err(err) => return internal(&err),
    };
    let path = normalize_path(parts.uri.path());

    // RP-003 y RP-007: parámetros capturados por Axum, o `AllowedMethods` si el
    // verbo no está en la ruta. El fallback del núcleo decide 404 o 405.
    let core_req = http::Request::from_parts(parts, bytes);
    let (handler, request) = match entry.handler_for(&method) {
        Some(handler) => (
            Arc::clone(handler),
            with_extensions(core_req, params.into_iter().collect(), None),
        ),
        None => (
            Arc::clone(&entry.fallback),
            with_extensions(
                core_req,
                Vec::new(),
                Some(AllowedMethods(entry.allowed.clone())),
            ),
        ),
    };

    let resp = invoke_guarded(handler, request, &path).await;
    let (parts, body) = resp.into_parts();
    Response::from_parts(parts, Body::from(body))
}

/// Handler del `fallback` de Axum: ninguna ruta coincidió → 404 (RP-007).
pub(crate) async fn serve_fallback(
    fallback: BoxHandler,
    limits: Limits,
    req: Request,
) -> Result<Response, Infallible> {
    let path = normalize_path(req.uri().path());
    let limit = limits.body_limit_bytes;
    let (mut parts, body) = req.into_parts();
    let bytes = match read_capped(body, limit).await {
        Ok(bytes) => bytes,
        Err(ReadBody::TooLarge) => return Ok(payload_too_large(limit, &path)),
        Err(ReadBody::Failed(err)) => return Ok(internal(&err)),
    };
    parts.uri = match normalize_uri(&parts.uri) {
        Ok(uri) => uri,
        Err(err) => return Ok(internal(&err)),
    };
    let path = normalize_path(parts.uri.path());
    let resp = invoke_guarded(fallback, http::Request::from_parts(parts, bytes), &path).await;
    let (parts, body) = resp.into_parts();
    Ok(Response::from_parts(parts, Body::from(body)))
}
