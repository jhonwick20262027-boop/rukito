//! U-08 (§20, §26.13): `--upload` con `UPLOAD_DRIVER=memory`.
//!
//! Levanta el binario real en un puerto efímero y comprueba subida, metadata,
//! borrado, aislamiento por dueño, 413 y que sin `--upload` las rutas no existen.

use std::process::Stdio;
use std::time::Duration;

use bytes::Bytes;
use http::{Method, StatusCode};
use http_body_util::{BodyExt, Full};
use hyper_util::rt::TokioIo;
use tokio::net::{TcpListener, TcpStream};
use tokio::process::{Child, Command};
use tokio::time::timeout;

const WAIT: Duration = Duration::from_secs(20);
const SECRET: &str = "secreto-de-prueba-lo-bastante-largo-para-hs256";
const PASSWORD: &str = "contrasena-de-prueba";
/// Suficiente para los cuerpos JSON del registro y el login.
const LIMIT: usize = 4096;

struct Demo {
    child: Child,
    addr: std::net::SocketAddr,
}

impl Demo {
    async fn start(upload: bool) -> Self {
        let port = {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            listener.local_addr().unwrap().port()
        };
        let mut command = Command::new(env!("CARGO_BIN_EXE_todo-app"));
        if upload {
            command.arg("--upload");
        }
        let child = command
            .env("RDD_MEMORY", "1")
            .env("JWT_SECRET", SECRET)
            .env("HOST", "127.0.0.1")
            .env("PORT", port.to_string())
            .env("UPLOAD_DRIVER", "memory")
            .env("UPLOAD_MAX_BYTES", LIMIT.to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .expect("no arrancó todo-app");
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
        method: Method,
        uri: &str,
        headers: &[(&str, &str)],
        body: Bytes,
    ) -> (StatusCode, Bytes) {
        let stream = TcpStream::connect(self.addr).await.expect("sin conexión");
        let (mut sender, conn) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
            .await
            .expect("handshake");
        tokio::spawn(async move {
            let _ = conn.await;
        });
        let mut request = http::Request::builder().method(method).uri(uri);
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        let request = request.body(Full::new(body)).expect("petición");
        let resp = sender.send_request(request).await.expect("sin respuesta");
        let (parts, body) = resp.into_parts();
        let body = timeout(WAIT, body.collect())
            .await
            .expect("la respuesta no terminó")
            .expect("cuerpo ilegible")
            .to_bytes();
        (parts.status, body)
    }

    /// Registra y autentica una cuenta; devuelve su JWT.
    async fn token(&self, email: &str) -> String {
        let credentials = format!(r#"{{"email":"{email}","password":"{PASSWORD}"}}"#);
        let headers = [("content-type", "application/json")];

        let (status, _) = self
            .send(
                Method::POST,
                "/auth/register",
                &headers,
                Bytes::from(credentials.clone()),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "registro de {email}");

        let (status, body) = self
            .send(
                Method::POST,
                "/auth/login",
                &headers,
                Bytes::from(credentials),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "login de {email}");
        let json: serde_json::Value = serde_json::from_slice(&body).expect("login en JSON");
        json["access_token"]
            .as_str()
            .expect("el login devuelve un JWT")
            .to_owned()
    }

    async fn stop(mut self) {
        let _ = self.child.kill().await;
    }
}

fn json(body: &Bytes) -> serde_json::Value {
    serde_json::from_slice(body).expect("el cuerpo debe ser JSON")
}

#[tokio::test]
async fn u08_sube_consulta_borra_y_aisla_por_dueno() {
    let demo = Demo::start(true).await;
    let ana = demo.token("ana@example.com").await;
    let bob = demo.token("bob@example.com").await;
    let ana_auth = format!("Bearer {ana}");
    let bob_auth = format!("Bearer {bob}");

    // POST /files: 201 con id, tamaño y tipo; el nombre llega por cabecera.
    let (status, body) = demo
        .send(
            Method::POST,
            "/files",
            &[
                ("authorization", &ana_auth),
                ("x-file-name", "notas.txt"),
                ("content-type", "text/plain"),
            ],
            Bytes::from_static(b"hola"),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let created = json(&body);
    let id = created["id"]
        .as_str()
        .expect("201 devuelve el id")
        .to_owned();
    uuid::Uuid::parse_str(&id).expect("el id es un UUID del servidor");
    assert_eq!(created["size"], 4);
    assert_eq!(created["content_type"], "text/plain");

    // GET /files/:id: la misma metadata, sin bytes.
    let uri = format!("/files/{id}");
    let (status, body) = demo
        .send(
            Method::GET,
            &uri,
            &[("authorization", &ana_auth)],
            Bytes::new(),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let meta = json(&body);
    assert_eq!(meta["id"], created["id"]);
    assert_eq!(meta["size"], created["size"]);
    assert_eq!(meta["content_type"], created["content_type"]);

    // El dueño B no lee ni borra el archivo de A: 404, no 403.
    let (status, body) = demo
        .send(
            Method::GET,
            &uri,
            &[("authorization", &bob_auth)],
            Bytes::new(),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(json(&body)["status"], 404);

    let (status, _) = demo
        .send(
            Method::DELETE,
            &uri,
            &[("authorization", &bob_auth)],
            Bytes::new(),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // DELETE propio: 204 y el GET posterior es 404.
    let (status, body) = demo
        .send(
            Method::DELETE,
            &uri,
            &[("authorization", &ana_auth)],
            Bytes::new(),
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(body.is_empty(), "204 no lleva cuerpo");

    let (status, _) = demo
        .send(
            Method::GET,
            &uri,
            &[("authorization", &ana_auth)],
            Bytes::new(),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Un cuerpo mayor que UPLOAD_MAX_BYTES es 413 antes del handler (RF-017).
    let (status, _) = demo
        .send(
            Method::POST,
            "/files",
            &[
                ("authorization", &ana_auth),
                ("x-file-name", "grande.bin"),
                ("content-type", "application/octet-stream"),
            ],
            Bytes::from(vec![b'x'; LIMIT + 1]),
        )
        .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);

    // Sin sesión no hay subida (RequireAuth).
    let (status, _) = demo
        .send(
            Method::POST,
            "/files",
            &[("x-file-name", "notas.txt"), ("content-type", "text/plain")],
            Bytes::from_static(b"hola"),
        )
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    demo.stop().await;
}

#[tokio::test]
async fn u08_sin_upload_las_rutas_no_existen() {
    let demo = Demo::start(false).await;
    let token = demo.token("ana@example.com").await;
    let auth = format!("Bearer {token}");

    for (method, uri) in [
        (Method::POST, "/files".to_owned()),
        (Method::GET, format!("/files/{}", uuid::Uuid::new_v4())),
        (Method::DELETE, format!("/files/{}", uuid::Uuid::new_v4())),
    ] {
        let (status, _) = demo
            .send(
                method.clone(),
                &uri,
                &[
                    ("authorization", &auth),
                    ("x-file-name", "notas.txt"),
                    ("content-type", "text/plain"),
                ],
                Bytes::from_static(b"hola"),
            )
            .await;
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "{method} {uri} no debe existir sin --upload"
        );
    }

    demo.stop().await;
}

/// La cabecera `content-type` es obligatoria en `POST /files`: sin ella el
/// extractor `Header<String>` responde 400 (RF-054).
#[tokio::test]
async fn u08_sin_content_type_es_400() {
    let demo = Demo::start(true).await;
    let token = demo.token("ana@example.com").await;
    let auth = format!("Bearer {token}");

    let (status, _) = demo
        .send(
            Method::POST,
            "/files",
            &[("authorization", &auth), ("x-file-name", "notas.txt")],
            Bytes::from_static(b"hola"),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    demo.stop().await;
}
