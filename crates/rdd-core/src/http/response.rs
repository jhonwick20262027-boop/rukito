//! Conversión de valores y errores a respuestas del núcleo (§10, RF-052, RF-072).

use bytes::Bytes;
use http::header::{self, CONTENT_TYPE};
use http::{Response, StatusCode};

use crate::error::{Error, Problem};
use crate::http::types::{HeaderMap, Request};

/// Métodos permitidos en una ruta, puesto en las extensiones por la plataforma
/// cuando el método no coincide (RP-007).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AllowedMethods(pub Vec<http::Method>);

impl AllowedMethods {
    /// Valor vacío: la ruta no existe en absoluto → 404.
    pub fn none() -> Self {
        Self(Vec::new())
    }

    /// Cabecera `Allow` como lista separada por comas (RF-072).
    pub(crate) fn allow_header(&self) -> Option<String> {
        if self.0.is_empty() {
            return None;
        }
        let mut names: Vec<String> = self.0.iter().map(|m| m.as_str().to_owned()).collect();
        names.sort();
        names.dedup();
        Some(names.join(", "))
    }
}

/// Error de infraestructura materializado en 500, para el fallback de `compose`.
pub fn internal_error(instance: &str, err: &Error) -> Response<Bytes> {
    tracing::error!(error = %err, path = instance, "fallo interno del framework");
    let problem = Problem::from_error(
        &Error::Internal(anyhow::anyhow!(
            "no se pudo construir la respuesta de error"
        )),
        instance,
    );
    problem_response(StatusCode::INTERNAL_SERVER_ERROR, &problem, None)
}

/// Lee el cuerpo de una petición como bytes (RP-002: ya está en memoria).
pub fn body_to_bytes(req: &Request) -> Bytes {
    req.body().clone()
}

/// Construye una respuesta Problem Details (RF-072, §10.3).
pub fn problem_response(
    status: StatusCode,
    problem: &Problem,
    allow: Option<String>,
) -> Response<Bytes> {
    let json = serde_json::to_vec(problem)
        .unwrap_or_else(|_| b"{\"type\":\"about:blank\",\"status\":500}".to_vec());
    let mut builder = http::Response::builder()
        .status(status)
        .header(CONTENT_TYPE, "application/problem+json");
    if let Some(allow) = allow {
        builder = builder.header(header::ALLOW, allow);
    }
    builder
        .body(Bytes::from(json))
        .unwrap_or_else(|_| fallback_problem(status))
}

fn fallback_problem(status: StatusCode) -> Response<Bytes> {
    let mut res = Response::new(Bytes::from(
        "{\"type\":\"about:blank\",\"status\":500,\"title\":\"Internal Server Error\"}",
    ));
    *res.status_mut() = status;
    res.headers_mut().insert(
        CONTENT_TYPE,
        http::HeaderValue::from_static("application/problem+json"),
    );
    res
}

/// Convierte valores del handler en respuesta.
///
/// Todo lo serializable se convierte en JSON con el estado de la ruta (RF-052);
/// 204 y 304 se emiten sin cuerpo, que es lo que la macro fija cuando el handler
/// devuelve `()` (RF-053).
pub trait IntoResponse {
    /// Convierte `self` en respuesta. `status` es el código base de la ruta.
    fn into_response(self, status: StatusCode) -> Response<Bytes>;
}

/// Cualquier valor serializable es una respuesta JSON con el estado de la ruta
/// (RF-052). Es una impl blanket a propósito: `Result<Task>`, `Result<Vec<Task>>`
/// y `Result<String>` funcionan sin envoltorios en los handlers.
impl<T> IntoResponse for T
where
    T: serde::Serialize,
{
    fn into_response(self, status: StatusCode) -> Response<Bytes> {
        // RF-053: `()` llega aquí con estado 204; RFC 9110 prohíbe cuerpo en 204
        // y 304, así que se serializa vacío en vez de `null`.
        if status == StatusCode::NO_CONTENT || status == StatusCode::NOT_MODIFIED {
            return http::Response::builder()
                .status(status)
                .body(Bytes::new())
                .unwrap_or_else(|_| Response::new(Bytes::new()));
        }
        json_response(status, &self)
    }
}

/// Serializa un valor como `application/json` (RF-070).
pub fn json_response<T: serde::Serialize + ?Sized>(
    status: StatusCode,
    value: &T,
) -> Response<Bytes> {
    match serde_json::to_vec(value) {
        Ok(body) => http::Response::builder()
            .status(status)
            .header(CONTENT_TYPE, "application/json")
            .body(Bytes::from(body))
            .unwrap_or_else(|_| server_error_response()),
        Err(err) => internal_error_response(&err, "/"),
    }
}

/// Punto único de conversión `(status, Result<T>)` → respuesta (RF-052, RF-072).
///
/// Un `Err` se convierte siempre en Problem Details; el estado del error manda
/// sobre el estado de la ruta, salvo que el handler devuelva `Ok`.
pub fn into_response<T>(status: StatusCode, result: crate::Result<T>) -> Response<Bytes>
where
    T: IntoResponse,
{
    match result {
        Ok(value) => value.into_response(status),
        Err(err) => error_response(status, &err),
    }
}

/// Convierte un error ya materializado a Problem Details.
pub fn error_response(_status: StatusCode, err: &Error) -> Response<Bytes> {
    if err.is_internal() {
        // RF-041: el detalle va al log, nunca al cliente.
        tracing::error!(error = %err, "fallo interno no expuesto al cliente");
    }
    let problem = Problem::from_error(err, "/");
    problem_response(err.status(), &problem, None)
}

/// Respuesta de error con la ruta real en `instance` (§10.3).
///
/// Es la conversión que usa `materialize`: el `Err` de un middleware o de un
/// handler acaba aquí y nunca llega a la plataforma (RP-004).
pub fn error_response_for(err: &Error, instance: &str) -> Response<Bytes> {
    if err.is_internal() {
        // RF-041: el detalle va al log, nunca al cliente.
        tracing::error!(error = %err, path = instance, "fallo interno no expuesto al cliente");
    }
    let problem = Problem::from_error(err, instance);
    problem_response(err.status(), &problem, None)
}

/// 500 con `detail` genérico para fallos del propio framework.
pub fn internal_error_response(err: &dyn std::error::Error, instance: &str) -> Response<Bytes> {
    tracing::error!(error = %err, "fallo del framework al serializar la respuesta");
    let problem = Problem {
        r#type: "about:blank".to_owned(),
        title: "Internal Server Error".to_owned(),
        status: StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
        detail: "ha ocurrido un error interno".to_owned(),
        instance: instance.to_owned(),
    };
    problem_response(StatusCode::INTERNAL_SERVER_ERROR, &problem, None)
}

fn server_error_response() -> Response<Bytes> {
    internal_error_response(&SimpleError("no se pudo construir la respuesta"), "/")
}

/// Error mínimo para rutas de construcción de respuesta que no pueden fallar.
#[derive(Debug)]
struct SimpleError(&'static str);

impl std::fmt::Display for SimpleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for SimpleError {}

/// Construye un [`HeaderMap`] vacío reutilizable.
pub fn empty_headers() -> HeaderMap {
    HeaderMap::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Serialize;

    #[derive(Serialize)]
    struct Tarea {
        id: u32,
        titulo: String,
    }

    fn body_of(resp: &Response<Bytes>) -> serde_json::Value {
        serde_json::from_slice(resp.body().as_ref()).unwrap()
    }

    #[test]
    fn problem_details_tiene_los_cinco_campos_de_rfc_9457() {
        let err = Error::not_found("la tarea 3 no existe");
        let resp = error_response_for(&err, "/tasks/3");
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            resp.headers().get(CONTENT_TYPE).unwrap(),
            "application/problem+json"
        );
        let body = body_of(&resp);
        assert_eq!(body["type"], "about:blank");
        assert_eq!(body["title"], "Not Found");
        assert_eq!(body["status"], 404);
        assert_eq!(body["detail"], "la tarea 3 no existe");
        assert_eq!(body["instance"], "/tasks/3");
    }

    #[test]
    fn el_detalle_interno_no_llega_al_cliente() {
        // RF-041.
        let err = Error::internal(anyhow::anyhow!("cadena de conexión corrupta"));
        let resp = error_response_for(&err, "/tasks");
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = serde_json::from_slice::<serde_json::Value>(resp.body().as_ref()).unwrap();
        assert_eq!(body["detail"], "ha ocurrido un error interno");
        assert!(!body.to_string().contains("cadena de conexión"));
    }

    #[test]
    fn cada_error_tipico_mapea_a_su_estado() {
        // §10.3.
        assert_eq!(Error::bad_request("x").status(), StatusCode::BAD_REQUEST);
        assert_eq!(Error::Unauthorized.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(Error::Forbidden.status(), StatusCode::FORBIDDEN);
        assert_eq!(Error::not_found("x").status(), StatusCode::NOT_FOUND);
        assert_eq!(Error::conflict("x").status(), StatusCode::CONFLICT);
        assert_eq!(
            Error::invalid("x").status(),
            StatusCode::UNPROCESSABLE_ENTITY
        );
        assert_eq!(
            Error::internal(anyhow::anyhow!("x")).status(),
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }

    #[test]
    fn all_10_3_codes_use_problem_details() {
        // §10.3: recorrido completo, con cuerpo y cabecera, no solo `status()`.
        let cases = [
            (Error::bad_request("x"), 400u16, "Bad Request"),
            (Error::Unauthorized, 401, "Unauthorized"),
            (Error::Forbidden, 403, "Forbidden"),
            (Error::not_found("x"), 404, "Not Found"),
            (Error::conflict("x"), 409, "Conflict"),
            (Error::invalid("x"), 422, "Unprocessable Entity"),
            (
                Error::internal(anyhow::anyhow!("x")),
                500,
                "Internal Server Error",
            ),
        ];

        for (err, status, title) in cases {
            let resp = error_response_for(&err, "/x");
            assert_eq!(resp.status().as_u16(), status, "{title}");
            assert_eq!(
                resp.headers().get(CONTENT_TYPE).unwrap(),
                "application/problem+json",
                "{title}"
            );
            let body = body_of(&resp);
            assert_eq!(body["type"], "about:blank", "{title}");
            assert_eq!(body["title"], title, "{title}");
            assert_eq!(body["status"], status, "{title}");
            assert_eq!(body["instance"], "/x", "{title}");
            assert!(body["detail"].is_string(), "{title}");
        }
    }

    #[test]
    fn el_204_va_sin_cuerpo() {
        // RF-053: la macro fija 204 cuando el handler devuelve `()`; aquí se
        // comprueba que ese estado nunca lleva cuerpo, aunque `()` sea serializable.
        let resp = ().into_response(StatusCode::NO_CONTENT);
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        assert!(resp.body().is_empty());
    }

    #[test]
    fn una_lista_se_serializa_como_json() {
        // RF-052, RF-070.
        let resp = vec![Tarea {
            id: 1,
            titulo: "pan".to_owned(),
        }]
        .into_response(StatusCode::OK);
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get(CONTENT_TYPE).unwrap(),
            "application/json"
        );
        assert_eq!(body_of(&resp)[0]["id"], 1);
    }

    #[test]
    fn cualquier_serializable_es_el_cuerpo_json() {
        // RF-052 sin envoltorios: el handler devuelve el dominio, no un tipo de
        // respuesta. Un `Option::None` es `null`, no 404; el 404 lo produce el
        // `Error::not_found` del handler.
        let resp = Some(Tarea {
            id: 7,
            titulo: "café".to_owned(),
        })
        .into_response(StatusCode::OK);
        assert_eq!(body_of(&resp)["id"], 7);

        let resp = None::<Tarea>.into_response(StatusCode::OK);
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(body_of(&resp).is_null());
    }

    #[test]
    fn post_sin_estado_explicito_es_201() {
        // RF-053 aplicado desde `api::default_status`.
        let resp = crate::api::default_status(&http::Method::POST);
        assert_eq!(resp, StatusCode::CREATED);
        assert_eq!(
            crate::api::default_status(&http::Method::GET),
            StatusCode::OK
        );
    }

    #[test]
    fn el_405_lleva_cabecera_allow_ordenada() {
        // RF-072, C-02.
        let permitidos = AllowedMethods(vec![http::Method::DELETE, http::Method::GET]);
        let allow = permitidos.allow_header().unwrap();
        assert_eq!(allow, "DELETE, GET");
        assert_eq!(AllowedMethods::none().allow_header(), None);
    }
}
