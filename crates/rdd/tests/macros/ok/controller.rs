//! Caso bueno de `#[controller]` y `module!` (§8.2, §8.5, §6.2).
//!
//! Una prueba `pass` de trybuild compila **y ejecuta** el archivo, así que aquí
//! se verifica también lo que produce la expansión: verbos, rutas normalizadas,
//! estados, extractores y el orden de middlewares.

use rdd::http::Method;
use rdd::prelude::*;
use rdd::{controller, module};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Task {
    id: u32,
    title: String,
}

#[derive(Debug, Deserialize)]
struct Filter {
    q: String,
}

#[derive(Clone)]
struct Tenant(String);

/// Middleware de controller: deja `Tenant` en las extensiones para que lo lea
/// `Ext<Tenant>` (RF-083).
fn tenant() -> impl Middleware {
    fn_middleware(|mut req, next| async move {
        let value = req
            .headers()
            .get("x-tenant")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("anon")
            .to_owned();
        req.extensions_mut().insert(Tenant(value));
        next.run(req).await
    })
}

/// Middleware de ruta: corta con 403 sin invocar al handler (RF-082).
fn deny() -> impl Middleware {
    fn_middleware(|_req, _next| async move { Err(Error::Forbidden) })
}

pub struct TaskController;

#[controller("/tasks")]
#[middleware(tenant())]
impl TaskController {
    #[get("/")]
    async fn list(filter: Query<Filter>) -> Result<Vec<Task>> {
        Ok(vec![Task {
            id: 1,
            title: filter.q.clone(),
        }])
    }

    #[get("/:id")]
    async fn one(id: Path<u32>, tenant: Ext<Tenant>) -> Result<Task> {
        Ok(Task {
            id: *id,
            title: tenant.0.clone(),
        })
    }

    #[post("/", status = 202)]
    async fn create(body: Json<Task>) -> Result<Task> {
        Ok(body.into_inner())
    }

    #[delete("/:id")]
    #[middleware(deny())]
    async fn remove(_id: Path<u32>) -> Result<()> {
        Ok(())
    }
}

fn request(method: Method, uri: &str, tenant: Option<&str>, body: &str) -> Request {
    let mut builder = http::Request::builder().method(method).uri(uri);
    if let Some(tenant) = tenant {
        builder = builder.header("x-tenant", tenant);
    }
    builder
        .body(bytes::Bytes::copy_from_slice(body.as_bytes()))
        .unwrap()
}

fn json(resp: &Response) -> serde_json::Value {
    serde_json::from_slice(resp.body().as_ref()).unwrap()
}

#[rdd::main]
async fn main() -> Result<()> {
    let routes = TaskController::routes();
    let table: Vec<String> = routes
        .iter()
        .map(|def| format!("{} {}", def.method, def.path))
        .collect();
    // RF-051: el prefijo se normaliza (`"/tasks"` + `"/"` → `/tasks`) y RF-056:
    // `routes()` devuelve las `RouteDef` generadas.
    assert_eq!(
        table,
        [
            "GET /tasks",
            "GET /tasks/:id",
            "POST /tasks",
            "DELETE /tasks/:id",
        ],
        "la macro no generó las rutas del spec"
    );

    let built = App::new()
        .module(module! { controllers: [TaskController], middlewares: [] })
        .without_health()
        .build()?;
    let http = &built.http;

    // Query<T> + estado 200 por defecto.
    let resp = http.dispatch(request(Method::GET, "/tasks?q=pan", None, "")).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(json(&resp)[0]["title"], "pan");

    // Path<T> convertido con FromStr y Ext<T> puesto por el middleware de controller.
    let resp = http
        .dispatch(request(Method::GET, "/tasks/7", Some("acme"), ""))
        .await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(json(&resp), serde_json::json!({ "id": 7, "title": "acme" }));

    // RF-055: un parámetro que no convierte es 400 en Problem Details.
    let resp = http.dispatch(request(Method::GET, "/tasks/abc", None, "")).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        resp.headers()
            .get(http::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok()),
        Some("application/problem+json")
    );

    // Json<T> + `status = 202` explícito (RF-053).
    let resp = http
        .dispatch(request(
            Method::POST,
            "/tasks",
            None,
            r#"{"id":2,"title":"café"}"#,
        ))
        .await;
    assert_eq!(resp.status(), StatusCode::ACCEPTED);
    assert_eq!(json(&resp)["title"], "café");

    // El middleware de ruta corta antes del handler, y el de controller ya pasó.
    let resp = http
        .dispatch(request(Method::DELETE, "/tasks/7", Some("acme"), ""))
        .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    // RF-072: método no permitido → 405 con `Allow`.
    let resp = http.dispatch(request(Method::PUT, "/tasks/7", None, "")).await;
    assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert!(
        resp.headers()
            .get(http::header::ALLOW)
            .is_some_and(|v| v.to_str().unwrap().contains("DELETE")),
        "falta la cabecera Allow"
    );

    Ok(())
}
