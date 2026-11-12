//! O-01 a O-06 (§20, §10.10): documento OpenAPI 3.1 y montaje `/api-docs`.
//!
//! Cada caso levanta un `AxumPlatform` real en `127.0.0.1:0` con la feature
//! `openapi` y apaga con `ShutdownSignal`; los que esperan fallo de arranque
//! comprueban el error que devuelve `serve` antes de escuchar.

#![cfg(feature = "openapi")]

use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper_util::rt::TokioIo;
use rdd::AxumPlatform;
use rdd::http::{
    HeaderMap, HttpApp, Platform, Response, RouteDef, ShutdownSignal, StatusCode, header,
};
use rdd::openapi::{OpenApi, OpenApiBuilder, OpenApiHttp, OpenApiHttpConfig, Operation};
use rdd::{App, Error};
use serde_json::Value;
use tokio::net::{TcpListener, TcpStream};
use tokio::time::timeout;

const WAIT: Duration = Duration::from_secs(10);

fn document() -> OpenApi {
    OpenApiBuilder::new("todo-app", "0.1.0")
        .description("Demo de rdd")
        .operation(
            "get",
            "/tasks",
            Operation::new("listTasks", "Lista las tareas").response(200, "ok"),
        )
        .build()
}

fn http_app() -> HttpApp {
    App::new()
        .without_health()
        .build()
        .expect("la app de prueba debe construirse")
        .http
}

fn http_app_with_route(path: &str) -> HttpApp {
    let mut http = http_app();
    let handler: rdd::http::BoxHandler = Arc::new(|_req| {
        Box::pin(async {
            let mut resp = Response::new(Bytes::from_static(b"ok"));
            *resp.status_mut() = StatusCode::OK;
            resp
        })
    });
    http.routes
        .push(RouteDef::new(http::Method::GET, path, Arc::clone(&handler)));
    http
}

struct Running {
    addr: std::net::SocketAddr,
    shutdown: ShutdownSignal,
    task: tokio::task::JoinHandle<rdd::error::PlatformResult<()>>,
}

impl Running {
    async fn stop(self) {
        self.shutdown.trigger();
        match timeout(WAIT, self.task).await {
            Err(_) => panic!("la plataforma no cerró dentro de {WAIT:?}"),
            Ok(Err(err)) => panic!("la tarea del servidor entró en pánico: {err}"),
            Ok(Ok(Err(err))) => panic!("la plataforma terminó con error: {err}"),
            Ok(Ok(Ok(()))) => {}
        }
    }
}

async fn start(config: OpenApiHttpConfig, app: HttpApp) -> Running {
    let openapi = OpenApiHttp::new(document()).config(config);
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("no se pudo vincular el puerto efímero");
    let addr = listener.local_addr().expect("socket sin dirección");
    let shutdown = ShutdownSignal::new();
    let serving = Arc::new(AxumPlatform::new().openapi(openapi))
        .serve(app, listener, shutdown.clone())
        .unwrap_or_else(|err| panic!("el montaje /api-docs no arrancó: {err}"));
    let task = tokio::spawn(serving);
    Running {
        addr,
        shutdown,
        task,
    }
}

struct Reply {
    status: StatusCode,
    headers: HeaderMap,
    body: Bytes,
}

async fn send(addr: std::net::SocketAddr, req: http::Request<Full<Bytes>>) -> Reply {
    let stream = TcpStream::connect(addr)
        .await
        .unwrap_or_else(|err| panic!("no se pudo conectar a {addr}: {err}"));
    let (mut sender, conn) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
        .await
        .expect("falló el handshake HTTP/1.1");
    tokio::spawn(async move {
        let _ = conn.await;
    });
    let resp = sender
        .send_request(req)
        .await
        .unwrap_or_else(|err| panic!("la petición no obtuvo respuesta: {err}"));
    let (parts, body) = resp.into_parts();
    let body = timeout(WAIT, body.collect())
        .await
        .expect("la respuesta no terminó a tiempo")
        .expect("no se pudo leer el cuerpo")
        .to_bytes();
    Reply {
        status: parts.status,
        headers: parts.headers,
        body,
    }
}

fn request(method: &str, path: &str) -> http::Request<Full<Bytes>> {
    http::Request::builder()
        .method(method)
        .uri(path)
        .body(Full::new(Bytes::new()))
        .expect("petición de prueba inválida")
}

#[tokio::test]
async fn o01_la_ui_se_sirve_en_api_docs_con_csp_y_no_store() {
    let running = start(OpenApiHttpConfig::new(), http_app_with_route("/tasks")).await;
    let reply = send(running.addr, request("GET", "/api-docs")).await;

    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(
        reply.headers[header::CONTENT_TYPE],
        "text/html; charset=utf-8"
    );
    assert_eq!(
        reply.headers[header::CONTENT_SECURITY_POLICY],
        "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; \
         connect-src 'self'"
    );
    assert_eq!(reply.headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
    assert_eq!(reply.headers[header::CACHE_CONTROL], "no-store");

    let html = String::from_utf8_lossy(&reply.body);
    assert!(html.contains("id=\"rdd-openapi-document\""));
    assert!(html.contains("\"openapi\":\"3.1.0\""));
    assert!(!html.contains("__RDD_OPENAPI_DOCUMENT__"));

    running.stop().await;
}

#[tokio::test]
async fn o02_alias_y_json_sirven_el_mismo_documento() {
    let running = start(OpenApiHttpConfig::new(), http_app_with_route("/tasks")).await;

    for alias in ["/docs", "/swagger"] {
        let reply = send(running.addr, request("GET", alias)).await;
        assert_eq!(reply.status, StatusCode::OK, "{alias}");
        assert_eq!(
            reply.headers[header::CONTENT_TYPE],
            "text/html; charset=utf-8",
            "{alias}"
        );
    }

    let reply = send(running.addr, request("GET", "/api-docs/openapi.json")).await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(
        reply.headers[header::CONTENT_TYPE],
        "application/json; charset=utf-8"
    );
    let doc: Value = serde_json::from_slice(&reply.body).expect("el JSON no es válido");
    assert_eq!(doc["openapi"], "3.1.0");
    assert_eq!(doc["info"]["title"], "todo-app");
    assert_eq!(doc["paths"]["/tasks"]["get"]["operationId"], "listTasks");

    running.stop().await;
}

#[tokio::test]
async fn o03_solo_get_y_rutas_conocidas() {
    let running = start(OpenApiHttpConfig::new(), http_app_with_route("/tasks")).await;

    let reply = send(running.addr, request("POST", "/api-docs")).await;
    assert_eq!(reply.status, StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(
        reply.headers[header::CONTENT_TYPE],
        "application/problem+json"
    );
    assert_eq!(reply.headers[header::ALLOW], "GET");

    let reply = send(running.addr, request("GET", "/api-docs/otra")).await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND);

    running.stop().await;
}

#[tokio::test]
async fn o04_authorize_protege_la_ui_salvo_public_docs() {
    let authorize = OpenApiHttpConfig::new().authorize(|req| {
        if req.headers().contains_key(header::AUTHORIZATION) {
            Ok(())
        } else {
            Err(Error::Unauthorized)
        }
    });
    let running = start(authorize, http_app_with_route("/tasks")).await;

    let reply = send(running.addr, request("GET", "/api-docs")).await;
    assert_eq!(reply.status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        reply.headers[header::CONTENT_TYPE],
        "application/problem+json"
    );

    let mut authed = request("GET", "/api-docs");
    authed
        .headers_mut()
        .insert(header::AUTHORIZATION, "Bearer demo".parse().unwrap());
    let reply = send(running.addr, authed).await;
    assert_eq!(reply.status, StatusCode::OK);

    running.stop().await;

    let public = OpenApiHttpConfig::new()
        .public_docs(true)
        .authorize(|_req| Err(Error::Unauthorized));
    let running = start(public, http_app_with_route("/tasks")).await;
    let reply = send(running.addr, request("GET", "/api-docs")).await;
    assert_eq!(reply.status, StatusCode::OK);
    let reply = send(running.addr, request("GET", "/api-docs/openapi.json")).await;
    assert_eq!(reply.status, StatusCode::OK);

    running.stop().await;
}

#[tokio::test]
async fn o05_el_documento_debe_cubrir_las_rutas_montadas() {
    // Ruta montada sin documentar: falla con las rutas estrictas.
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("no se pudo vincular el puerto efímero");
    let shutdown = ShutdownSignal::new();
    let Err(err) = Arc::new(AxumPlatform::new().openapi(OpenApiHttp::new(document()))).serve(
        http_app_with_route("/tareas"),
        listener,
        shutdown,
    ) else {
        panic!("un documento que no cubre las rutas montadas debe impedir arrancar");
    };
    let message = err.to_string();
    assert!(message.contains("GET /tareas"), "{message}");

    // Ruta documentada que no existe: falla incluso sin rutas estrictas.
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("no se pudo vincular el puerto efímero");
    let shutdown = ShutdownSignal::new();
    let relaxed = OpenApiHttpConfig::new().strict_routes(false);
    let Err(err) = Arc::new(
        AxumPlatform::new().openapi(OpenApiHttp::new(document()).config(relaxed)),
    )
    .serve(http_app(), listener, shutdown) else {
        panic!("una ruta documentada inexistente debe impedir arrancar");
    };
    let message = err.to_string();
    assert!(message.contains("GET /tasks"), "{message}");
}

#[tokio::test]
async fn o06_el_montaje_no_colisiona_con_la_app() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("no se pudo vincular el puerto efímero");
    let shutdown = ShutdownSignal::new();
    let Err(err) = Arc::new(AxumPlatform::new().openapi(OpenApiHttp::new(document()))).serve(
        http_app_with_route("/api-docs"),
        listener,
        shutdown,
    ) else {
        panic!("la documentación no puede ocupar una ruta HTTP existente");
    };
    let message = err.to_string();
    assert!(message.contains("/api-docs"), "{message}");
}
