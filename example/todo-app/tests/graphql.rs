//! G-09 (§10.9, §26.9): el demo `--graphql` con dos usuarios.
//!
//! Levanta el binario real en un puerto efímero, registra dos cuentas por REST,
//! consulta y crea tareas por `/graphql` con el JWT de cada una y comprueba que
//! la suscripción `taskChanged` solo entrega los eventos del dueño del token.

use std::process::Stdio;
use std::time::Duration;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
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
            .arg("--graphql")
            .env("RDD_MEMORY", "1")
            .env("JWT_SECRET", SECRET)
            .env("HOST", "127.0.0.1")
            .env("PORT", port.to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .expect("no arrancó todo-app --graphql");
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

    async fn send(&self, req: http::Request<Full<Bytes>>) -> (http::StatusCode, Bytes) {
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
        (parts.status, body)
    }

    async fn send_stream(
        &self,
        req: http::Request<Full<Bytes>>,
    ) -> (http::StatusCode, http::HeaderMap, Incoming) {
        let stream = TcpStream::connect(self.addr).await.expect("sin conexión");
        let (mut sender, conn) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
            .await
            .expect("handshake");
        tokio::spawn(async move {
            let _ = conn.await;
        });
        let resp = sender.send_request(req).await.expect("sin respuesta");
        let (parts, body) = resp.into_parts();
        (parts.status, parts.headers, body)
    }

    async fn register(&self, email: &str) {
        let (status, body) = self
            .send(json_post(
                "/auth/register",
                &json!({ "email": email, "password": PASSWORD }),
                None,
            ))
            .await;
        assert_eq!(
            status,
            201,
            "register falló: {}",
            String::from_utf8_lossy(&body)
        );
    }

    async fn login(&self, email: &str) -> String {
        let (status, body) = self
            .send(json_post(
                "/auth/login",
                &json!({ "email": email, "password": PASSWORD }),
                None,
            ))
            .await;
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

    /// `POST /graphql` autenticado; devuelve el JSON de la respuesta.
    async fn gql(&self, token: &str, query: &str, variables: Value) -> Value {
        let (status, body) = self
            .send(gql_post(
                token,
                &json!({ "query": query, "variables": variables }),
                "application/json",
            ))
            .await;
        assert_eq!(
            status,
            200,
            "graphql falló: {}",
            String::from_utf8_lossy(&body)
        );
        serde_json::from_slice(&body).expect("respuesta GraphQL no es JSON")
    }

    async fn rest_create(&self, token: &str, title: &str) -> String {
        let (status, body) = self
            .send(json_post("/tasks", &json!({ "title": title }), Some(token)))
            .await;
        assert_eq!(
            status,
            201,
            "crear por REST falló: {}",
            String::from_utf8_lossy(&body)
        );
        let body: Value = serde_json::from_slice(&body).unwrap();
        body["id"].as_str().expect("sin id de la tarea").to_owned()
    }

    async fn finish(mut self) {
        let _ = self.child.start_kill();
        let _ = timeout(WAIT, self.child.wait()).await;
    }
}

fn json_post(uri: &str, value: &Value, token: Option<&str>) -> http::Request<Full<Bytes>> {
    let mut builder = http::Request::builder()
        .method("POST")
        .uri(uri)
        .header(http::header::CONTENT_TYPE, "application/json");
    if let Some(token) = token {
        builder = builder.header(http::header::AUTHORIZATION, format!("Bearer {token}"));
    }
    builder
        .body(Full::new(Bytes::from(value.to_string())))
        .unwrap()
}

fn gql_post(token: &str, value: &Value, accept: &str) -> http::Request<Full<Bytes>> {
    http::Request::builder()
        .method("POST")
        .uri("/graphql")
        .header(http::header::CONTENT_TYPE, "application/json")
        .header(http::header::ACCEPT, accept)
        .header(http::header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Full::new(Bytes::from(value.to_string())))
        .unwrap()
}

fn explorer(token: Option<&str>) -> http::Request<Full<Bytes>> {
    let mut builder = http::Request::builder()
        .method("GET")
        .uri("/graphql")
        .header(http::header::ACCEPT, "text/html");
    if let Some(token) = token {
        builder = builder.header(http::header::AUTHORIZATION, format!("Bearer {token}"));
    }
    builder.body(Full::new(Bytes::new())).unwrap()
}

async fn next_block(body: &mut Incoming, pending: &mut Vec<u8>) -> String {
    loop {
        if let Some(position) = pending.windows(2).position(|pair| pair == b"\n\n") {
            let block: Vec<u8> = pending.drain(..position + 2).collect();
            return String::from_utf8(block).expect("SSE no es UTF-8");
        }
        let frame = timeout(WAIT, body.frame())
            .await
            .expect("el flujo SSE no emitió nada")
            .expect("el flujo SSE terminó")
            .expect("marco inválido");
        if let Ok(data) = frame.into_data() {
            pending.extend_from_slice(&data);
        }
    }
}

fn data_of(block: &str) -> Option<Value> {
    let data = block.lines().find_map(|line| line.strip_prefix("data: "))?;
    Some(serde_json::from_str(data).expect("data no es JSON"))
}

fn titles(value: &Value) -> Vec<String> {
    value["data"]["tasks"]
        .as_array()
        .expect("sin arreglo tasks")
        .iter()
        .map(|task| task["title"].as_str().unwrap().to_owned())
        .collect()
}

#[tokio::test]
async fn g09_dos_usuarios_y_suscripcion() {
    let demo = Demo::start().await;
    demo.register("ana@correo.com").await;
    demo.register("beto@correo.com").await;
    let ana = demo.login("ana@correo.com").await;
    let beto = demo.login("beto@correo.com").await;

    // La identidad va antes que el protocolo: sin Bearer no hay explorer, no hay
    // consulta y no hay suscripción (RF-239).
    let (status, _) = demo.send(explorer(None)).await;
    assert_eq!(status, 401, "explorer sin token");
    let anonymous = http::Request::builder()
        .method("POST")
        .uri("/graphql")
        .header(http::header::CONTENT_TYPE, "application/json")
        .header(http::header::ACCEPT, "application/json")
        .body(Full::new(Bytes::from(
            json!({ "query": "{ tasks { id } }" }).to_string(),
        )))
        .unwrap();
    let (status, _) = demo.send(anonymous).await;
    assert_eq!(status, 401, "consulta sin token");
    let anonymous = http::Request::builder()
        .method("POST")
        .uri("/graphql")
        .header(http::header::CONTENT_TYPE, "application/json")
        .header(http::header::ACCEPT, "text/event-stream")
        .body(Full::new(Bytes::from(
            json!({ "query": "subscription { taskChanged { id } }" }).to_string(),
        )))
        .unwrap();
    let (status, _) = demo.send(anonymous).await;
    assert_eq!(status, 401, "suscripción sin token");

    // El explorer se sirve embebido y sin recursos externos (RF-240).
    let (status, body) = demo.send(explorer(Some(&ana))).await;
    assert_eq!(status, 200);
    let html = String::from_utf8(body.to_vec()).unwrap();
    assert!(html.contains("<!doctype html") || html.contains("<!DOCTYPE html"));
    assert!(!html.contains("src=\"http"));

    // Crear por mutación: el dueño sale de la sesión, nunca del cuerpo.
    let created = demo
        .gql(
            &ana,
            "mutation($title: String!) { createTask(input: { title: $title }) { id title done } }",
            json!({ "title": "de ana" }),
        )
        .await;
    assert!(created.get("errors").is_none(), "sin errores: {created}");
    assert_eq!(created["data"]["createTask"]["title"], "de ana");
    let ana_task = created["data"]["createTask"]["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let created = demo
        .gql(
            &beto,
            "mutation($title: String!) { createTask(input: { title: $title }) { id } }",
            json!({ "title": "de beto" }),
        )
        .await;
    let beto_task = created["data"]["createTask"]["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let listed = demo.gql(&ana, "{ tasks { id title } }", json!({})).await;
    assert_eq!(listed["data"]["tasks"][0]["id"], ana_task.as_str());
    assert_eq!(titles(&listed), ["de ana"], "ana solo ve lo suyo");
    let listed = demo.gql(&beto, "{ tasks { title } }", json!({})).await;
    assert_eq!(titles(&listed), ["de beto"], "beto solo ve lo suyo");

    // Completar la tarea del otro es un 404 de dominio, no una fuga.
    let foreign = demo
        .gql(
            &ana,
            "mutation($id: ID!) { completeTask(id: $id) { id done } }",
            json!({ "id": beto_task }),
        )
        .await;
    let errors = foreign["errors"].as_array().expect("sin errors");
    let message = errors[0]["message"].as_str().unwrap();
    assert!(
        message.contains("no existe la tarea"),
        "mensaje inesperado: {message}"
    );

    let completed = demo
        .gql(
            &beto,
            "mutation($id: ID!) { completeTask(id: $id) { id done } }",
            json!({ "id": beto_task }),
        )
        .await;
    assert_eq!(completed["data"]["completeTask"]["done"], true);

    // La suscripción de ana se abre antes de que existan los eventos.
    let (status, headers, mut body) = demo
        .send_stream(gql_post(
            &ana,
            &json!({ "query": "subscription { taskChanged { id title } }" }),
            "text/event-stream",
        ))
        .await;
    assert_eq!(status, 200);
    let content_type = headers
        .get(http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    assert!(
        content_type.starts_with("text/event-stream"),
        "content-type: {content_type}"
    );

    let mut pending = Vec::new();
    let ping = next_block(&mut body, &mut pending).await;
    assert_eq!(ping, ": ping\n\n", "primer bloque SSE");

    // El REST sigue siendo el camino de escritura que alimenta el hub; primero
    // crea beto y después ana, y solo el segundo evento debe salir.
    demo.rest_create(&beto, "rest de beto").await;
    let rest_ana = demo.rest_create(&ana, "rest de ana").await;

    let event = next_block(&mut body, &mut pending).await;
    assert!(event.contains("event: next"), "bloque inesperado: {event}");
    let payload = data_of(&event).expect("evento sin data");
    assert_eq!(payload["data"]["taskChanged"]["id"], rest_ana.as_str());
    assert_eq!(payload["data"]["taskChanged"]["title"], "rest de ana");

    // Nada del otro usuario viaja en el mismo flujo: no hay otro bloque a la
    // vista (los latidos llegan cada 15 s, muy lejos de esta espera).
    let extra = timeout(Duration::from_millis(500), body.frame()).await;
    assert!(extra.is_err(), "llegó un evento que no era de ana");

    drop(body);
    demo.finish().await;
}
