//! M-16 (§20, §26.8): el demo `--mcp-http` con dos usuarios.
//!
//! Levanta el binario real en un puerto efímero, registra dos cuentas por REST,
//! inicializa una sesión `/mcp` por usuario y comprueba que las tools solo ven
//! las tareas del dueño del token.

use std::process::Stdio;
use std::time::Duration;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper_util::rt::TokioIo;
use serde_json::{Value, json};
use tokio::net::{TcpListener, TcpStream};
use tokio::process::{Child, Command};
use tokio::time::timeout;

const WAIT: Duration = Duration::from_secs(20);
const SECRET: &str = "secreto-de-prueba-lo-bastante-largo-para-hs256";
const PASSWORD: &str = "larga123";

struct Demo {
    child: Child,
    addr: std::net::SocketAddr,
}

impl Demo {
    async fn start() -> Self {
        let port = {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            listener.local_addr().unwrap().port()
        };
        let child = Command::new(env!("CARGO_BIN_EXE_todo-app"))
            .arg("--mcp-http")
            .env("RDD_MEMORY", "1")
            .env("JWT_SECRET", SECRET)
            .env("HOST", "127.0.0.1")
            .env("PORT", port.to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .expect("no arrancó todo-app --mcp-http");
        let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));

        timeout(WAIT, async {
            loop {
                if TcpStream::connect(addr).await.is_ok() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .expect("el demo no abrió el puerto a tiempo");

        Self { child, addr }
    }

    async fn send(
        &self,
        req: http::Request<Full<Bytes>>,
    ) -> (http::StatusCode, http::HeaderMap, Bytes) {
        let stream = TcpStream::connect(self.addr).await.expect("sin conexión");
        let (mut sender, conn) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
            .await
            .expect("handshake");
        tokio::spawn(async move {
            let _ = conn.await;
        });
        let resp = sender.send_request(req).await.expect("sin respuesta");
        let (parts, body) = resp.into_parts();
        let body = timeout(WAIT, body.collect())
            .await
            .expect("respuesta sin fin")
            .expect("cuerpo ilegible")
            .to_bytes();
        (parts.status, parts.headers, body)
    }

    async fn register(&self, email: &str) {
        let req = post(
            "/auth/register",
            &json!({ "email": email, "password": PASSWORD }),
            &[],
        );
        let (status, _, body) = self.send(req).await;
        assert_eq!(
            status,
            201,
            "register falló: {}",
            String::from_utf8_lossy(&body)
        );
    }

    async fn login(&self, email: &str) -> String {
        let req = post(
            "/auth/login",
            &json!({ "email": email, "password": PASSWORD }),
            &[],
        );
        let (status, _, body) = self.send(req).await;
        assert_eq!(
            status,
            200,
            "login falló: {}",
            String::from_utf8_lossy(&body)
        );
        let body: Value = serde_json::from_slice(&body).unwrap();
        body["access_token"]
            .as_str()
            .expect("sin access_token")
            .to_owned()
    }

    async fn init_mcp(&self, token: &str) -> String {
        let req = mcp(
            None,
            &json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-06-18",
                    "capabilities": {},
                    "clientInfo": { "name": "m16", "version": "1" }
                }
            }),
            Some(token),
            &[],
        );
        let (status, headers, body) = self.send(req).await;
        assert_eq!(
            status,
            200,
            "initialize falló: {}",
            String::from_utf8_lossy(&body)
        );
        headers
            .get("mcp-session-id")
            .and_then(|value| value.to_str().ok())
            .expect("sin Mcp-Session-Id")
            .to_owned()
    }

    async fn call(&self, session: &str, token: &str, id: u64, name: &str, args: Value) -> Value {
        let req = mcp(
            Some(session),
            &json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": "tools/call",
                "params": { "name": name, "arguments": args }
            }),
            Some(token),
            &[],
        );
        let (status, _, body) = self.send(req).await;
        assert_eq!(
            status,
            200,
            "tools/call `{name}`: {}",
            String::from_utf8_lossy(&body)
        );
        let response: Value = serde_json::from_slice(&body).unwrap();
        assert!(
            response.get("error").is_none(),
            "tools/call `{name}` falló: {response}"
        );
        let text = response["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("sin texto: {response}"));
        serde_json::from_str(text).unwrap_or_else(|_| panic!("texto no JSON: {text:?}"))
    }

    async fn finish(mut self) {
        let _ = self.child.start_kill();
        let _ = timeout(WAIT, self.child.wait()).await;
    }
}

fn post(
    uri: &str,
    value: &Value,
    extra: &[(&'static str, &'static str)],
) -> http::Request<Full<Bytes>> {
    let mut builder = http::Request::builder()
        .method("POST")
        .uri(uri)
        .header(http::header::CONTENT_TYPE, "application/json");
    for (name, value) in extra {
        builder = builder.header(*name, *value);
    }
    builder
        .body(Full::new(Bytes::from(value.to_string())))
        .unwrap()
}

fn mcp(
    session: Option<&str>,
    value: &Value,
    token: Option<&str>,
    extra: &[(&'static str, &'static str)],
) -> http::Request<Full<Bytes>> {
    let mut builder = http::Request::builder()
        .method("POST")
        .uri("/mcp")
        .header(http::header::CONTENT_TYPE, "application/json")
        .header(http::header::ACCEPT, "application/json, text/event-stream");
    if let Some(session) = session {
        builder = builder.header("mcp-session-id", session);
    }
    if let Some(token) = token {
        builder = builder.header(http::header::AUTHORIZATION, format!("Bearer {token}"));
    }
    for (name, value) in extra {
        builder = builder.header(*name, *value);
    }
    builder
        .body(Full::new(Bytes::from(value.to_string())))
        .unwrap()
}

fn delete(session: &str, token: &str) -> http::Request<Full<Bytes>> {
    http::Request::builder()
        .method("DELETE")
        .uri("/mcp")
        .header("mcp-session-id", session)
        .header(http::header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Full::new(Bytes::new()))
        .unwrap()
}

#[tokio::test]
async fn m16_dos_usuarios_solo_ven_sus_tareas() {
    let demo = Demo::start().await;
    demo.register("ana@correo.com").await;
    demo.register("beto@correo.com").await;
    let ana = demo.login("ana@correo.com").await;
    let beto = demo.login("beto@correo.com").await;

    // Sin Bearer, authorize corta antes del protocolo (RF-231).
    let anonymous = mcp(
        None,
        &json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": { "name": "m16", "version": "1" }
            }
        }),
        None,
        &[],
    );
    let (status, _, body) = demo.send(anonymous).await;
    assert_eq!(status, 401, "sin token: {}", String::from_utf8_lossy(&body));

    let session_ana = demo.init_mcp(&ana).await;
    let session_beto = demo.init_mcp(&beto).await;

    let created = demo
        .call(
            &session_ana,
            &ana,
            2,
            "create_task",
            json!({ "title": "de ana" }),
        )
        .await;
    assert_eq!(created["title"], "de ana");
    let ana_task = created["id"].as_str().unwrap().to_owned();

    let created = demo
        .call(
            &session_beto,
            &beto,
            2,
            "create_task",
            json!({ "title": "de beto" }),
        )
        .await;
    assert_eq!(created["title"], "de beto");

    let listed = demo
        .call(&session_ana, &ana, 3, "list_tasks", json!({}))
        .await;
    let titles: Vec<&str> = listed
        .as_array()
        .expect("arreglo de tareas")
        .iter()
        .map(|task| task["title"].as_str().unwrap())
        .collect();
    assert_eq!(titles, ["de ana"], "ana solo ve lo suyo");

    let listed = demo
        .call(&session_beto, &beto, 3, "list_tasks", json!({}))
        .await;
    let titles: Vec<&str> = listed
        .as_array()
        .expect("arreglo de tareas")
        .iter()
        .map(|task| task["title"].as_str().unwrap())
        .collect();
    assert_eq!(titles, ["de beto"], "beto solo ve lo suyo");

    let completed = demo
        .call(
            &session_ana,
            &ana,
            4,
            "complete_task",
            json!({ "id": ana_task }),
        )
        .await;
    assert_eq!(completed["done"], true);

    // Un origen de navegador no listado se rechaza antes de las tools (RF-228).
    let bad_origin = mcp(
        Some(&session_ana),
        &json!({ "jsonrpc": "2.0", "id": 5, "method": "ping" }),
        Some(&ana),
        &[("origin", "https://malo.example")],
    );
    let (status, _, _) = demo.send(bad_origin).await;
    assert_eq!(status, 403);

    // DELETE deja la sesión en 404 (RF-223).
    let (status, _, body) = demo.send(delete(&session_ana, &ana)).await;
    assert_eq!(status, 204, "DELETE: {}", String::from_utf8_lossy(&body));
    let after = mcp(
        Some(&session_ana),
        &json!({ "jsonrpc": "2.0", "id": 6, "method": "ping" }),
        Some(&ana),
        &[],
    );
    let (status, _, _) = demo.send(after).await;
    assert_eq!(status, 404);

    demo.finish().await;
}
