//! M-10 a M-15 (§20, §10.8): el montaje `/mcp` contra un `AxumPlatform` real.
//!
//! Cada caso levanta la app en `127.0.0.1:0`, habla HTTP de verdad y apaga con
//! la misma `ShutdownSignal` que usa la suite de conformidad.

use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper_util::rt::TokioIo;
use rdd_axum::AxumPlatform;
use rdd_core::api::{Router, route_def};
use rdd_core::http::{
    Ctx, FromContainer, HeaderMap, Middleware, NextFn, Platform, Request, RequestExt,
    ShutdownSignal, StatusCode, fn_middleware, header,
};
use rdd_core::{App, Controller, Error};
use rdd_mcp::{CallToolResult, McpHttp, McpHttpConfig, McpServer, Tool, ToolContext};
use serde_json::{Value, json};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::timeout;

const WAIT: Duration = Duration::from_secs(10);
const BODY_LIMIT: usize = 1024;

// --- app de prueba -------------------------------------------------------

struct Routes;

impl Controller for Routes {
    const NAME: &'static str = "mcp-suite";

    fn register(router: &mut Router) {
        router.push(route_def(
            rdd_core::http::Method::GET,
            "/tasks",
            StatusCode::OK,
            Vec::new(),
            |_: Ctx| async move { Ok(json!({ "tasks": [] })) },
        ));
    }
}

/// Identidad mínima: lo que un `JwtMiddleware` deja en las extensiones.
#[derive(Clone)]
struct Identity {
    email: String,
}

/// Servicio del contenedor, para comprobar `ctx.resolve` (RF-228).
#[derive(Clone)]
struct Greeting(String);

impl FromContainer for Greeting {
    const NAME: &'static str = "Greeting";

    fn from_container(container: &rdd_core::di::Container) -> Result<Self, Error> {
        container
            .get::<Self>()
            .ok_or_else(|| Error::not_found("Greeting"))
    }
}

fn seed_identity() -> impl Middleware {
    fn_middleware(|mut req: Request, next: NextFn| async move {
        let bearer = req
            .headers()
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok());
        if bearer == Some("Bearer ana") {
            req.extensions_mut().insert(Identity {
                email: "ana@correo.com".to_owned(),
            });
        }
        next.run(req).await
    })
}

fn require_identity(req: &Request) -> rdd_core::Result<()> {
    req.ext::<Identity>().map(|_| ()).ok_or(Error::Unauthorized)
}

fn base_app(identity: bool) -> rdd_core::http::HttpApp {
    let app = App::new()
        .without_health()
        .body_limit(BODY_LIMIT)
        .provide(Greeting("hola".to_owned()))
        .controller::<Routes>();
    let app = if identity {
        app.middleware(seed_identity())
    } else {
        app
    };
    app.build().expect("la app de prueba debe construirse").http
}

struct EchoCtx;

impl Tool for EchoCtx {
    fn name(&self) -> &str {
        "echo_ctx"
    }

    fn description(&self) -> &str {
        "Devuelve la identidad y el servicio vistos por la tool."
    }

    fn input_schema(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }

    fn call(
        &self,
        _args: Value,
        ctx: ToolContext,
    ) -> rdd_core::http::BoxFuture<'static, rdd_core::Result<CallToolResult>> {
        Box::pin(async move {
            let identity = ctx.ext::<Identity>().ok_or(Error::Unauthorized)?;
            let greeting = ctx.resolve::<Greeting>()?;
            Ok(CallToolResult::text(format!(
                "{}|{}",
                identity.email, greeting.0
            )))
        })
    }
}

struct Progress;

impl Tool for Progress {
    fn name(&self) -> &str {
        "progress"
    }

    fn description(&self) -> &str {
        "Emite dos progresos y termina."
    }

    fn input_schema(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }

    fn call(
        &self,
        _args: Value,
        ctx: ToolContext,
    ) -> rdd_core::http::BoxFuture<'static, rdd_core::Result<CallToolResult>> {
        Box::pin(async move {
            ctx.progress(1.0, Some(2.0))?;
            ctx.progress(2.0, Some(2.0))?;
            Ok(CallToolResult::text("listo"))
        })
    }
}

struct Slow;

impl Tool for Slow {
    fn name(&self) -> &str {
        "slow"
    }

    fn description(&self) -> &str {
        "Tarda hasta que la cancelan."
    }

    fn input_schema(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }

    fn call(
        &self,
        _args: Value,
        ctx: ToolContext,
    ) -> rdd_core::http::BoxFuture<'static, rdd_core::Result<CallToolResult>> {
        Box::pin(async move {
            tokio::select! {
                _ = ctx.cancelled() => Err(Error::invalid("cancelada")),
                _ = tokio::time::sleep(Duration::from_secs(30)) => {
                    Ok(CallToolResult::text("tarde"))
                }
            }
        })
    }
}

fn mcp_server() -> McpServer {
    McpServer::new("suite", "0.1.0")
        .tool(EchoCtx)
        .unwrap()
        .tool(Progress)
        .unwrap()
        .tool(Slow)
        .unwrap()
}

struct Running {
    addr: std::net::SocketAddr,
    shutdown: ShutdownSignal,
    task: tokio::task::JoinHandle<rdd_core::error::PlatformResult<()>>,
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

async fn start(app: rdd_core::http::HttpApp, config: McpHttpConfig) -> Running {
    let mcp = McpHttp::new(mcp_server()).config(config);
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("no se pudo vincular el puerto efímero");
    let addr = listener.local_addr().expect("socket sin dirección");
    let shutdown = ShutdownSignal::new();
    let serving = Arc::new(AxumPlatform::new().mcp(mcp))
        .serve(app, listener, shutdown.clone())
        .unwrap_or_else(|err| panic!("el montaje /mcp no arrancó: {err}"));
    let task = tokio::spawn(serving);
    Running {
        addr,
        shutdown,
        task,
    }
}

// --- cliente HTTP --------------------------------------------------------

struct Reply {
    status: StatusCode,
    headers: HeaderMap,
    body: Bytes,
}

async fn connect(
    addr: std::net::SocketAddr,
) -> hyper::client::conn::http1::SendRequest<Full<Bytes>> {
    let stream = TcpStream::connect(addr)
        .await
        .unwrap_or_else(|err| panic!("no se pudo conectar a {addr}: {err}"));
    let (sender, conn) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
        .await
        .expect("falló el handshake HTTP/1.1");
    tokio::spawn(async move {
        let _ = conn.await;
    });
    sender
}

async fn send(addr: std::net::SocketAddr, req: http::Request<Full<Bytes>>) -> Reply {
    let mut sender = connect(addr).await;
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

async fn send_stream(
    addr: std::net::SocketAddr,
    req: http::Request<Full<Bytes>>,
) -> (StatusCode, HeaderMap, Incoming) {
    let mut sender = connect(addr).await;
    let resp = sender
        .send_request(req)
        .await
        .unwrap_or_else(|err| panic!("la petición SSE no obtuvo respuesta: {err}"));
    let (parts, body) = resp.into_parts();
    (parts.status, parts.headers, body)
}

fn rpc(session: Option<&str>, value: &Value) -> http::Request<Full<Bytes>> {
    rpc_with(session, value, "application/json, text/event-stream", &[])
}

fn rpc_with(
    session: Option<&str>,
    value: &Value,
    accept: &str,
    extra: &[(&'static str, &'static str)],
) -> http::Request<Full<Bytes>> {
    let mut builder = http::Request::builder()
        .method("POST")
        .uri("/mcp")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ACCEPT, accept);
    if let Some(session) = session {
        builder = builder.header("mcp-session-id", session);
    }
    for (name, value) in extra {
        builder = builder.header(*name, *value);
    }
    builder
        .body(Full::new(Bytes::from(value.to_string())))
        .unwrap()
}

fn delete(session: Option<&str>) -> http::Request<Full<Bytes>> {
    let mut builder = http::Request::builder().method("DELETE").uri("/mcp");
    if let Some(session) = session {
        builder = builder.header("mcp-session-id", session);
    }
    builder.body(Full::new(Bytes::new())).unwrap()
}

fn body_json(reply: &Reply) -> Value {
    serde_json::from_slice(&reply.body).unwrap_or_else(|_| {
        panic!(
            "cuerpo no JSON ({}): {:?}",
            reply.body.len(),
            String::from_utf8_lossy(&reply.body)
        )
    })
}

fn initialize(session: Option<&str>, version: &str) -> http::Request<Full<Bytes>> {
    rpc(
        session,
        &json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": version,
                "capabilities": {},
                "clientInfo": { "name": "m-suite", "version": "1" }
            }
        }),
    )
}

async fn open_session(addr: std::net::SocketAddr) -> String {
    let reply = send(addr, initialize(None, "2025-06-18")).await;
    assert_eq!(reply.status, StatusCode::OK, "initialize falló");
    reply
        .headers
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .expect("sin Mcp-Session-Id")
        .to_owned()
}

// --- M-10 ---------------------------------------------------------------

#[tokio::test]
async fn m10_initialize_crea_sesion_y_negocia_version() {
    let running = start(base_app(false), McpHttpConfig::new()).await;
    let addr = running.addr;

    let reply = send(addr, initialize(None, "2025-06-18")).await;
    assert_eq!(reply.status, StatusCode::OK);
    let sid = reply
        .headers
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .expect("falta Mcp-Session-Id")
        .to_owned();
    assert!(uuid_like(&sid), "el id de sesión debe ser un UUID: {sid}");
    assert_eq!(
        reply
            .headers
            .get("mcp-protocol-version")
            .and_then(|value| value.to_str().ok()),
        Some("2025-06-18")
    );
    let body = body_json(&reply);
    assert_eq!(body["result"]["protocolVersion"], "2025-06-18");
    assert!(body["result"]["capabilities"]["tools"].is_object());
    assert_eq!(body["result"]["serverInfo"]["name"], "suite");

    // RF-223: un segundo initialize dentro de la sesión viva es -32600.
    let reply = send(addr, initialize(Some(&sid), "2025-06-18")).await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(body_json(&reply)["error"]["code"], -32600);

    // RF-225: por HTTP, una versión no servida cae en la más reciente.
    let reply = send(addr, initialize(None, "2024-11-05")).await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(body_json(&reply)["result"]["protocolVersion"], "2025-11-25");

    running.stop().await;
}

fn uuid_like(value: &str) -> bool {
    value.len() == 36 && value.chars().filter(|c| *c == '-').count() == 4
}

// --- M-11 ---------------------------------------------------------------

#[tokio::test]
async fn m11_ciclo_de_sesion() {
    let running = start(
        base_app(false),
        McpHttpConfig::new().session_ttl(Duration::from_millis(80)),
    )
    .await;
    let addr = running.addr;

    let ping = rpc(
        None,
        &json!({ "jsonrpc": "2.0", "id": 2, "method": "ping" }),
    );
    let reply = send(addr, ping).await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST, "sin sesión es 400");

    let unknown = rpc(
        Some("00000000-0000-4000-8000-000000000000"),
        &json!({ "jsonrpc": "2.0", "id": 3, "method": "ping" }),
    );
    let reply = send(addr, unknown).await;
    assert_eq!(
        reply.status,
        StatusCode::NOT_FOUND,
        "sesión inventada es 404"
    );

    let sid = open_session(addr).await;
    let reply = send(addr, delete(Some(&sid))).await;
    assert_eq!(reply.status, StatusCode::NO_CONTENT);
    let reply = send(
        addr,
        rpc(
            Some(&sid),
            &json!({ "jsonrpc": "2.0", "id": 4, "method": "ping" }),
        ),
    )
    .await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND, "tras DELETE es 404");

    let sid = open_session(addr).await;
    tokio::time::sleep(Duration::from_millis(150)).await;
    let reply = send(
        addr,
        rpc(
            Some(&sid),
            &json!({ "jsonrpc": "2.0", "id": 5, "method": "ping" }),
        ),
    )
    .await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND, "la sesión caducó");

    running.stop().await;
}

// --- M-12 ---------------------------------------------------------------

#[tokio::test]
async fn m12_transporte_y_errores_del_protocolo() {
    let running = start(base_app(false), McpHttpConfig::new()).await;
    let addr = running.addr;

    let wrong_type = http::Request::builder()
        .method("POST")
        .uri("/mcp")
        .header(header::CONTENT_TYPE, "text/plain")
        .body(Full::new(Bytes::from_static(b"{}")))
        .unwrap();
    assert_eq!(
        send(addr, wrong_type).await.status,
        StatusCode::UNSUPPORTED_MEDIA_TYPE
    );

    let too_large = http::Request::builder()
        .method("POST")
        .uri("/mcp")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Full::new(Bytes::from(vec![b' '; BODY_LIMIT * 4])))
        .unwrap();
    assert_eq!(
        send(addr, too_large).await.status,
        StatusCode::PAYLOAD_TOO_LARGE
    );

    let no_accept = http::Request::builder()
        .method("POST")
        .uri("/mcp")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ACCEPT, "text/plain")
        .body(Full::new(Bytes::from_static(b"{}")))
        .unwrap();
    assert_eq!(
        send(addr, no_accept).await.status,
        StatusCode::NOT_ACCEPTABLE
    );

    let batched = rpc(
        None,
        &json!([{ "jsonrpc": "2.0", "id": 1, "method": "ping" }]),
    );
    let reply = send(addr, batched).await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST);
    assert_eq!(body_json(&reply)["error"]["code"], -32600);

    let broken = http::Request::builder()
        .method("POST")
        .uri("/mcp")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Full::new(Bytes::from_static(b"{no es json")))
        .unwrap();
    let reply = send(addr, broken).await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST);
    assert_eq!(body_json(&reply)["error"]["code"], -32700);

    let sid = open_session(addr).await;
    let notice = rpc(
        Some(&sid),
        &json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
    );
    let reply = send(addr, notice).await;
    assert_eq!(reply.status, StatusCode::ACCEPTED);
    assert!(reply.body.is_empty(), "202 sin cuerpo");

    let unknown = rpc(
        Some(&sid),
        &json!({ "jsonrpc": "2.0", "id": 9, "method": "inexistente" }),
    );
    let reply = send(addr, unknown).await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(body_json(&reply)["error"]["code"], -32601);

    for method in ["GET", "PUT"] {
        let req = http::Request::builder()
            .method(method)
            .uri("/mcp")
            .body(Full::new(Bytes::new()))
            .unwrap();
        let reply = send(addr, req).await;
        assert_eq!(reply.status, StatusCode::METHOD_NOT_ALLOWED, "{method}");
        assert_eq!(
            reply
                .headers
                .get(header::ALLOW)
                .and_then(|value| value.to_str().ok()),
            Some("DELETE, POST"),
            "{method}"
        );
    }

    running.stop().await;
}

// --- M-13 ---------------------------------------------------------------

#[tokio::test]
async fn m13_sse_con_progreso() {
    let running = start(base_app(false), McpHttpConfig::new()).await;
    let addr = running.addr;
    let sid = open_session(addr).await;

    let call = rpc_with(
        Some(&sid),
        &json!({
            "jsonrpc": "2.0",
            "id": 7,
            "method": "tools/call",
            "params": {
                "name": "progress",
                "arguments": {},
                "_meta": { "progressToken": "tok" }
            }
        }),
        "text/event-stream",
        &[],
    );
    let (status, headers, body) = send_stream(addr, call).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        headers
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("text/event-stream")
    );
    assert_eq!(
        headers
            .get(header::CACHE_CONTROL)
            .and_then(|value| value.to_str().ok()),
        Some("no-store")
    );

    let bytes = timeout(WAIT, body.collect())
        .await
        .expect("el flujo SSE no terminó")
        .expect("no se pudo leer el flujo SSE")
        .to_bytes();
    let events = sse_events(&bytes);
    assert!(events.len() >= 3, "faltan eventos: {events:?}");
    assert_eq!(events[0]["method"], "notifications/progress");
    assert_eq!(events[0]["params"]["progress"], 1.0);
    assert_eq!(events[1]["params"]["progress"], 2.0);
    let last = events.last().expect("sin mensaje final");
    assert_eq!(last["id"], 7);
    assert_eq!(last["result"]["content"][0]["text"], "listo");

    // RF-226: un progressToken sin `text/event-stream` es 406.
    let call = rpc_with(
        Some(&sid),
        &json!({
            "jsonrpc": "2.0",
            "id": 8,
            "method": "tools/call",
            "params": {
                "name": "progress",
                "arguments": {},
                "_meta": { "progressToken": "tok" }
            }
        }),
        "application/json",
        &[],
    );
    assert_eq!(send(addr, call).await.status, StatusCode::NOT_ACCEPTABLE);

    running.stop().await;
}

fn sse_events(bytes: &[u8]) -> Vec<Value> {
    let text = std::str::from_utf8(bytes).expect("SSE no es UTF-8");
    text.split("\n\n")
        .filter(|block| !block.trim().is_empty())
        .map(|block| {
            assert!(
                block.starts_with("event: message\n"),
                "evento sin `event: message`: {block:?}"
            );
            let data = block
                .lines()
                .find_map(|line| line.strip_prefix("data: "))
                .unwrap_or_else(|| panic!("evento sin `data:`: {block:?}"));
            serde_json::from_str(data).unwrap_or_else(|_| panic!("data no JSON: {data:?}"))
        })
        .collect()
}

// --- M-14 ---------------------------------------------------------------

#[tokio::test]
async fn m14_cancelacion_entre_posts() {
    let running = start(base_app(false), McpHttpConfig::new()).await;
    let addr = running.addr;
    let sid = open_session(addr).await;

    let slow = tokio::spawn(send(
        addr,
        rpc(
            Some(&sid),
            &json!({
                "jsonrpc": "2.0",
                "id": 42,
                "method": "tools/call",
                "params": { "name": "slow", "arguments": {} }
            }),
        ),
    ));
    tokio::time::sleep(Duration::from_millis(200)).await;

    let cancel = rpc(
        Some(&sid),
        &json!({
            "jsonrpc": "2.0",
            "method": "notifications/cancelled",
            "params": { "requestId": 42 }
        }),
    );
    let reply = send(addr, cancel).await;
    assert_eq!(reply.status, StatusCode::ACCEPTED);

    let reply = timeout(WAIT, slow)
        .await
        .expect("el POST cancelado no terminó")
        .expect("la tarea del POST entró en pánico");
    assert_eq!(reply.status, StatusCode::NO_CONTENT);
    assert!(
        reply.body.is_empty(),
        "el POST cancelado no entrega mensaje"
    );

    // La sesión sigue atendiendo (RF-227).
    let reply = send(
        addr,
        rpc(
            Some(&sid),
            &json!({ "jsonrpc": "2.0", "id": 43, "method": "ping" }),
        ),
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK);
    assert!(body_json(&reply)["result"].is_object());

    running.stop().await;
}

// --- M-15 ---------------------------------------------------------------

#[tokio::test]
async fn m15_entrada_identidad_y_origenes() {
    let app = App::new()
        .without_health()
        .body_limit(BODY_LIMIT)
        .provide(Greeting("hola".to_owned()))
        .middleware(seed_identity())
        .middleware(
            rdd_core::http::RateLimit::new(5, Duration::from_secs(60), |_req: &Request| {
                "m15".to_owned()
            })
            .expect("rate limit válido"),
        )
        .controller::<Routes>()
        .build()
        .expect("la app de prueba debe construirse")
        .http;

    let running = start(
        app,
        McpHttpConfig::new()
            .allowed_origins(["https://permitido.example"])
            .authorize(require_identity),
    )
    .await;
    let addr = running.addr;

    // Sin token, authorize corta antes del protocolo (1).
    let reply = send(addr, initialize(None, "2025-06-18")).await;
    assert_eq!(reply.status, StatusCode::UNAUTHORIZED);
    assert_eq!(body_json(&reply)["status"], 401);

    // Con token, initialize abre sesión (2).
    let reply = send(
        addr,
        rpc_with(
            None,
            &json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-06-18",
                    "capabilities": {},
                    "clientInfo": { "name": "m15", "version": "1" }
                }
            }),
            "application/json",
            &[("authorization", "Bearer ana")],
        ),
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK);
    let sid = reply
        .headers
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .expect("sin Mcp-Session-Id")
        .to_owned();

    // La tool ve la identidad y el servicio por el contexto (3).
    let reply = send(
        addr,
        rpc_with(
            Some(&sid),
            &json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "tools/call",
                "params": { "name": "echo_ctx", "arguments": {} }
            }),
            "application/json",
            &[("authorization", "Bearer ana")],
        ),
    )
    .await;
    let body = body_json(&reply);
    assert_eq!(body["result"]["content"][0]["text"], "ana@correo.com|hola");

    // Origen no permitido (4) y permitido (5).
    let bad = rpc_with(
        Some(&sid),
        &json!({ "jsonrpc": "2.0", "id": 3, "method": "ping" }),
        "application/json",
        &[
            ("authorization", "Bearer ana"),
            ("origin", "https://otro.example"),
        ],
    );
    assert_eq!(send(addr, bad).await.status, StatusCode::FORBIDDEN);

    let good = rpc_with(
        Some(&sid),
        &json!({ "jsonrpc": "2.0", "id": 4, "method": "ping" }),
        "application/json",
        &[
            ("authorization", "Bearer ana"),
            ("origin", "https://permitido.example"),
        ],
    );
    assert_eq!(send(addr, good).await.status, StatusCode::OK);

    // La siguiente petición supera el límite de 5 (6).
    let over = rpc_with(
        Some(&sid),
        &json!({ "jsonrpc": "2.0", "id": 5, "method": "ping" }),
        "application/json",
        &[("authorization", "Bearer ana")],
    );
    let reply = send(addr, over).await;
    assert_eq!(reply.status, StatusCode::TOO_MANY_REQUESTS);
    assert!(reply.headers.contains_key("retry-after"));

    running.stop().await;
}
