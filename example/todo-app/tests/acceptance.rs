//! Aceptación de 0.1 (§21, RF-007): el proyecto compila, responde en `/health`
//! y devuelve 404 y 405 en formato Problem Details (§10.3).
//!
//! Un test de integración no puede importar el binario, así que el árbol de
//! `src/` se incluye por ruta: `crate::modules::…` y `crate::app_*` resuelven
//! igual que en `main.rs` y no hay que duplicar el cableado.

#![allow(dead_code)]

#[path = "../src/app_controller.rs"]
mod app_controller;
#[path = "../src/app_module.rs"]
mod app_module;
#[path = "../src/app_service.rs"]
mod app_service;
#[path = "../src/modules/mod.rs"]
mod modules;

use rdd::http::{Method, header, into_response};
use rdd::prelude::*;
use rdd::{controller, module};

fn app() -> HttpApp {
    App::new()
        .in_memory()
        .module(app_module::module())
        .module(modules::tasks::module())
        .build()
        .expect("la app debe construirse")
        .http
}

async fn sqlite_app() -> HttpApp {
    let adapter =
        rdd::SqliteAdapter::connect_with("sqlite::memory:", 1, std::time::Duration::from_secs(5))
            .await
            .expect("SQLite debe conectar");
    sqlx::query(include_str!("../migrations/0001_tasks.sql"))
        .execute(adapter.pool())
        .await
        .expect("el esquema SQLite debe crearse");
    App::new()
        .adapter(adapter.into_arc())
        .module(app_module::module())
        .module(modules::tasks::module())
        .build()
        .expect("la app debe construirse")
        .http
}

fn request(method: Method, uri: &str, body: &str) -> Request {
    http::Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .body(bytes::Bytes::copy_from_slice(body.as_bytes()))
        .expect("petición válida")
}

async fn send(http: &HttpApp, method: Method, uri: &str, body: &str) -> Response {
    http.dispatch(request(method, uri, body)).await
}

fn json(resp: &Response) -> serde_json::Value {
    serde_json::from_slice(resp.body().as_ref()).expect("el cuerpo debe ser JSON")
}

/// §10.3: toda respuesta de error es `application/problem+json`.
fn problem(resp: &Response) -> serde_json::Value {
    assert_eq!(
        resp.headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("application/problem+json"),
    );
    json(resp)
}

#[tokio::test]
async fn health_responde_ok() {
    let http = app();
    let resp = send(&http, Method::GET, "/health", "").await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(json(&resp)["status"], "ok");
}

#[tokio::test]
async fn la_raiz_responde_json() {
    let http = app();
    let resp = send(&http, Method::GET, "/", "").await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("application/json"),
    );
}

#[tokio::test]
async fn ruta_inexistente_es_404_en_problem_details() {
    let http = app();
    let resp = send(&http, Method::GET, "/no-existe", "").await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body = problem(&resp);
    assert_eq!(body["status"], 404);
    assert_eq!(body["instance"], "/no-existe");
}

#[tokio::test]
async fn metodo_no_permitido_es_405_con_allow() {
    let http = app();
    let resp = send(&http, Method::POST, "/", "").await;
    assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert!(
        resp.headers()
            .get(header::ALLOW)
            .is_some_and(|value| value.to_str().unwrap().contains("GET")),
        "falta la cabecera Allow (RF-072)"
    );
    assert_eq!(problem(&resp)["status"], 405);
}

#[test]
fn todos_los_errores_salen_en_problem_details() {
    let cases = [
        (Error::bad_request("mal formado"), 400),
        (Error::Unauthorized, 401),
        (Error::Forbidden, 403),
        (Error::not_found("no existe"), 404),
        (Error::conflict("ya existe"), 409),
        (Error::invalid("no procesable"), 422),
    ];
    for (err, status) in cases {
        let resp = into_response(StatusCode::OK, Err::<(), _>(err));
        assert_eq!(resp.status().as_u16(), status);
        let body = problem(&resp);
        assert_eq!(body["type"], "about:blank");
        assert_eq!(body["status"], status);
        assert!(body["title"].is_string());
        assert!(body["detail"].is_string());
    }
}

/// Controller de prueba: verifica que un panic no tumba la app (RF-073).
struct BoomController;

#[controller("/boom")]
impl BoomController {
    #[get("/")]
    async fn boom() -> Result<()> {
        panic!("fallo provocado para verificar RF-073");
    }
}

#[tokio::test]
async fn un_panico_es_500_y_no_filtra_el_detalle() {
    let http = App::new()
        .module(module! { controllers: [BoomController], middlewares: [] })
        .build()
        .expect("la app debe construirse")
        .http;
    let resp = send(&http, Method::GET, "/boom", "").await;
    assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = problem(&resp);
    assert_eq!(body["status"], 500);
    // RF-041: el detalle interno va al log, no al cliente.
    assert!(
        !body["detail"]
            .as_str()
            .unwrap_or_default()
            .contains("provocado"),
        "el 500 no debe exponer el mensaje del panic"
    );
}

#[tokio::test]
async fn crud_del_modulo_de_ejemplo() {
    let http = app();

    let resp = send(&http, Method::POST, "/tasks", r#"{"title":"pan"}"#).await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let id = json(&resp)["id"]
        .as_str()
        .expect("el id debe ser un UUID")
        .to_owned();

    let resp = send(&http, Method::GET, &format!("/tasks/{id}"), "").await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(json(&resp)["title"], "pan");

    let resp = send(&http, Method::GET, "/tasks?q=pan", "").await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(json(&resp).as_array().unwrap().len(), 1);

    let done = r#"{"done":true}"#;
    let resp = send(&http, Method::PUT, &format!("/tasks/{id}"), done).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(json(&resp)["done"], true);

    let resp = send(&http, Method::DELETE, &format!("/tasks/{id}"), "").await;
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    assert!(resp.body().is_empty(), "204 no lleva cuerpo (RF-053)");

    let resp = send(&http, Method::GET, &format!("/tasks/{id}"), "").await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    assert_eq!(problem(&resp)["status"], 404);
}

#[tokio::test]
async fn crud_del_modulo_funciona_con_sqlite() {
    let http = sqlite_app().await;
    let resp = send(&http, Method::POST, "/tasks", r#"{"title":"persistir"}"#).await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let id = json(&resp)["id"].as_str().expect("UUID").to_owned();

    let resp = send(&http, Method::GET, &format!("/tasks/{id}"), "").await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(json(&resp)["title"], "persistir");

    let resp = send(
        &http,
        Method::PUT,
        &format!("/tasks/{id}"),
        r#"{"done":true}"#,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(json(&resp)["done"], true);

    let resp = send(&http, Method::DELETE, &format!("/tasks/{id}"), "").await;
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn validacion_del_modulo_de_ejemplo() {
    use crate::modules::tasks::constants::tasks_constants::MAX_TITLE_LEN;

    let http = app();

    let vacio = r#"{"title":"  "}"#;
    let resp = send(&http, Method::POST, "/tasks", vacio).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    assert_eq!(problem(&resp)["instance"], "/tasks");

    let largo = format!(r#"{{"title":"{}"}}"#, "x".repeat(MAX_TITLE_LEN + 1));
    let resp = send(&http, Method::POST, "/tasks", &largo).await;
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(problem(&resp)["status"], 422);

    // RF-055: un parámetro de ruta que no convierte es 400.
    let resp = send(&http, Method::GET, "/tasks/abc", "").await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    assert_eq!(problem(&resp)["status"], 400);
}
