//! Los casos de la suite (§4.5): C-02 a C-08, C-11 y C-12.
//!
//! Cada función lleva el ID del caso en el nombre y en los mensajes, para que un
//! fallo diga siempre qué requisito se rompió. La misma lista corre contra la
//! sonda de red y contra `dispatch` (RP-021).

use bytes::Bytes;
use rdd_core::http::{Method, Response, StatusCode};
use serde_json::Value;
use uuid::Uuid;

use crate::fixture::{BODY_LIMIT, SLOW_DELAY, counted_calls, reset_counter};
use crate::probe::{Probe, ProbeRequest, refuses_connections};

/// Construcción de petición: método + path, sin cuerpo.
fn request(method: Method, path: &str) -> ProbeRequest {
    http::Request::builder()
        .method(method)
        .uri(path)
        .body(Vec::new())
        .expect("la petición de la suite es válida")
}

/// Petición con cuerpo JSON crudo (para forzar JSON inválido también).
fn with_json(path: &str, body: &str) -> ProbeRequest {
    let mut req = request(Method::POST, path);
    *req.body_mut() = body.as_bytes().to_vec();
    req.headers_mut().insert(
        "content-type",
        http::HeaderValue::from_static("application/json"),
    );
    req
}

fn get(path: &str) -> ProbeRequest {
    request(Method::GET, path)
}

/// El cuerpo Problem Details ya parseado, con su cabecera verificada (§10.3).
fn problem(resp: &Response) -> Value {
    let content_type = resp
        .headers()
        .get("content-type")
        .map(|value| value.to_str().unwrap_or_default().to_owned())
        .unwrap_or_default();
    assert!(
        content_type.starts_with("application/problem+json"),
        "RF-072: un error DEBE ir en `application/problem+json`, llegó `{content_type}` con cuerpo `{}`",
        String::from_utf8_lossy(resp.body())
    );
    serde_json::from_slice(resp.body()).expect("el cuerpo Problem Details es JSON válido")
}

fn header<'a>(resp: &'a Response, name: &str) -> Option<&'a str> {
    resp.headers().get(name).and_then(|v| v.to_str().ok())
}

/// C-02 (RF-072, RP-007): 404 en Problem Details y 405 con `Allow`.
async fn c02_no_encontrada_y_metodo_no_permitido(probe: &Probe) {
    let resp = probe.send(get("/inexistente")).await;
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "C-02: una ruta que no existe debe dar 404"
    );
    let body = problem(&resp);
    assert_eq!(body["status"], 404, "C-02: `status` debe ser 404");
    assert_eq!(body["title"], "Not Found", "C-02: `title` de RFC 9457");
    assert_eq!(
        body["instance"], "/inexistente",
        "C-02: `instance` debe nombrar la ruta pedida"
    );

    // La ruta existe, pero no con DELETE.
    let resp = probe.send(request(Method::DELETE, "/tasks")).await;
    assert_eq!(
        resp.status(),
        StatusCode::METHOD_NOT_ALLOWED,
        "C-02: un verbo no registrado en una ruta existente debe dar 405"
    );
    let allow = header(&resp, "allow").unwrap_or_default().to_owned();
    for verb in ["GET", "POST"] {
        assert!(
            allow.split(',').any(|v| v.trim() == verb),
            "C-02 / RP-007: `Allow` debe listar los verbos registrados, llegó `{allow}`"
        );
    }
    assert_eq!(problem(&resp)["status"], 405, "C-02: `status` debe ser 405");
}

/// C-03 (RF-018, RP-008): `/tasks` y `/tasks/` responden igual.
async fn c03_barra_final(probe: &Probe) {
    let sin_barra = probe.send(get("/tasks")).await;
    let con_barra = probe.send(get("/tasks/")).await;

    assert_eq!(
        sin_barra.status(),
        StatusCode::OK,
        "C-03: `/tasks` debe responder 200"
    );
    assert_eq!(
        con_barra.status(),
        sin_barra.status(),
        "C-03: la barra final no cambia el estado"
    );
    assert_eq!(
        con_barra.body(),
        sin_barra.body(),
        "C-03: la barra final no cambia el cuerpo"
    );

    // Y con parámetros tampoco.
    let id = Uuid::new_v4();
    let con_barra = probe.send(get(&format!("/tasks/{id}/"))).await;
    assert_eq!(
        con_barra.status(),
        StatusCode::OK,
        "C-03: `/tasks/:id/` también coincide con `/tasks/:id`"
    );
}

/// C-04 (RF-055, RP-003): parámetros de ruta y UUID inválido.
async fn c04_parametros_de_ruta(probe: &Probe) {
    let id = Uuid::new_v4();

    let resp = probe.send(get(&format!("/tasks/{id}"))).await;
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "C-04: UUID válido debe leerse"
    );
    assert_eq!(
        serde_json::from_slice::<Value>(resp.body()).expect("JSON")["id"],
        id.to_string(),
        "C-04 / RP-003: la plataforma debe entregar el parámetro capturado al núcleo"
    );

    // Parámetro en el medio de la ruta.
    let resp = probe
        .send(request(Method::POST, &format!("/tasks/{id}/complete")))
        .await;
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "C-04: `/tasks/:id/complete` debe montar el parámetro"
    );

    // UUID inválido → 400 en Problem Details.
    let resp = probe.send(get("/tasks/no-es-un-uuid")).await;
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "C-04 / RF-055: un UUID inválido debe dar 400"
    );
    assert_eq!(problem(&resp)["status"], 400, "C-04: `status` debe ser 400");
}

/// C-05 (RF-017, RP-006): cuerpo mayor al límite → 413 sin invocar al handler.
async fn c05_limite_de_cuerpo(probe: &Probe) {
    reset_counter();

    let gigante = format!(r#"{{"title":"{}"}}"#, "x".repeat(BODY_LIMIT * 4));
    let resp = probe.send(with_json("/counted", &gigante)).await;
    assert_eq!(
        resp.status(),
        StatusCode::PAYLOAD_TOO_LARGE,
        "C-05: un cuerpo de {} bytes con límite de {BODY_LIMIT} debe dar 413",
        gigante.len()
    );
    assert_eq!(
        counted_calls(),
        0,
        "C-05 / RP-006: el handler no debe ejecutarse cuando el cuerpo supera el límite"
    );
    let body = problem(&resp);
    assert_eq!(body["status"], 413, "C-05: `status` debe ser 413");
    assert_eq!(
        body["instance"], "/counted",
        "C-05: `instance` debe nombrar la ruta"
    );

    // Contraprueba: con cuerpo pequeño el handler sí corre.
    let resp = probe.send(with_json("/counted", r#"{"title":"si"}"#)).await;
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "C-05: un cuerpo dentro del límite debe llegar al handler"
    );
    assert_eq!(
        counted_calls(),
        1,
        "C-05: el contador de la ruta medida solo cuenta ejecuciones reales"
    );
}

/// C-06 (RF-055): JSON inválido → 400.
async fn c06_json_invalido(probe: &Probe) {
    let resp = probe.send(with_json("/tasks", r#"{"title": }"#)).await;
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "C-06 / RF-055: un JSON inválido debe dar 400"
    );
    let body = problem(&resp);
    assert_eq!(body["status"], 400, "C-06: `status` debe ser 400");
    assert_eq!(
        body["instance"], "/tasks",
        "C-06: `instance` debe nombrar la ruta"
    );

    // Y el camino feliz, para que el 400 anterior no sea «siempre 400».
    let resp = probe.send(with_json("/tasks", r#"{"title":"pan"}"#)).await;
    assert_eq!(
        resp.status(),
        StatusCode::CREATED,
        "C-06: un JSON válido debe llegar al handler (201 en POST)"
    );
}

/// C-07 (RF-073, RP-010): `panic` en un handler → 500 y el servidor sigue vivo.
async fn c07_panico_aislado(probe: &Probe) {
    let resp = probe.send(get("/boom")).await;
    assert_eq!(
        resp.status(),
        StatusCode::INTERNAL_SERVER_ERROR,
        "C-07 / RF-073: un pánico del handler debe dar 500"
    );
    assert_eq!(
        problem(&resp)["status"],
        500,
        "C-07: el 500 también va en Problem Details"
    );

    // RP-010: una conexión fallida no detiene el servidor.
    let resp = probe.send(get("/tasks")).await;
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "C-07 / RP-010: después del pánico el servidor debe seguir respondiendo"
    );
}

/// C-08 (RF-081): el middleware global cubre las respuestas 404 y 405.
async fn c08_global_cubre_404_y_405(probe: &Probe) {
    let resp = probe.send(get("/tampoco-existe")).await;
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "C-08: la ruta no existe"
    );
    assert_eq!(
        header(&resp, "x-suite"),
        Some("presente"),
        "C-08 / RF-081: el middleware global debe envolver también el 404"
    );

    let resp = probe.send(request(Method::DELETE, "/tasks")).await;
    assert_eq!(
        resp.status(),
        StatusCode::METHOD_NOT_ALLOWED,
        "C-08: el verbo no está registrado"
    );
    assert_eq!(
        header(&resp, "x-suite"),
        Some("presente"),
        "C-08 / RF-081: el middleware global debe envolver también el 405"
    );
}

/// C-11 (RF-013, RP-009): apagado ordenado. Solo con sonda de red.
async fn c11_apagado_ordenado(probe: &Probe) {
    let slow = probe.clone();
    let in_course = tokio::spawn(async move { slow.send(get("/slow")).await });

    // Dejar que la petición lenta entre al handler antes de apagar.
    tokio::time::sleep(SLOW_DELAY / 3).await;
    probe.shutdown().trigger();
    tokio::time::sleep(SLOW_DELAY / 3).await;

    let resp = in_course
        .await
        .expect("la petición en curso no debe perderse al apagar");
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "C-11 / RP-009: una petición en curso debe terminar durante el drenaje"
    );

    assert!(
        refuses_connections(probe.addr()).await,
        "C-11 / RP-009: activado el apagado, la plataforma debe dejar de aceptar conexiones"
    );
}

/// C-12 (RP-010): 200 peticiones concurrentes responden correctamente.
async fn c12_concurrencia(probe: &Probe) {
    let mut tasks = Vec::with_capacity(200);
    for _ in 0..200 {
        let probe = probe.clone();
        tasks.push(tokio::spawn(async move { probe.send(get("/tasks")).await }));
    }

    let mut bodies = Vec::with_capacity(200);
    for task in tasks {
        let resp = task
            .await
            .expect("RP-010: una petición concurrente no debe tumbar su tarea");
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "C-12: toda petición concurrente debe responder 200"
        );
        bodies.push(resp.into_body());
    }

    let first = &bodies[0];
    for body in &bodies {
        assert_eq!(
            body, first,
            "C-12: el cuerpo de una petición concurrente no debe depender del orden"
        );
    }
    assert_eq!(
        first,
        &Bytes::from_static(br#"[{"id":1,"title":"pan"}]"#),
        "C-12: la lista esperada"
    );
}

/// Corre los casos disponibles para esta sonda.
///
/// `dispatch` no tiene socket, así que C-11 (apagado) solo aplica a la sonda de
/// red; los demás se ejecutan idénticos en ambas (RP-021).
pub(crate) async fn run_all_cases(probe: &Probe) {
    c02_no_encontrada_y_metodo_no_permitido(probe).await;
    c03_barra_final(probe).await;
    c04_parametros_de_ruta(probe).await;
    c05_limite_de_cuerpo(probe).await;
    c06_json_invalido(probe).await;
    c07_panico_aislado(probe).await;
    c08_global_cubre_404_y_405(probe).await;
    c12_concurrencia(probe).await;
    if probe.is_http() {
        c11_apagado_ordenado(probe).await;
    }
}
