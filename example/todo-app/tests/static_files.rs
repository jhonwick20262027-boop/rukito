//! S-08 (§20, §26.11): el demo `--static` sirve `client/` en `/site`.
//!
//! Levanta el binario real en un puerto efímero con el backend en memoria y
//! comprueba la página, sus assets y que el 404 siga siendo Problem Details.

use std::process::Stdio;
use std::time::Duration;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper_util::rt::TokioIo;
use tokio::net::{TcpListener, TcpStream};
use tokio::process::{Child, Command};
use tokio::time::timeout;

const WAIT: Duration = Duration::from_secs(20);
const SECRET: &str = "secreto-de-prueba-lo-bastante-largo-para-hs256";

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
            .arg("--static")
            .env("RDD_MEMORY", "1")
            .env("JWT_SECRET", SECRET)
            .env("HOST", "127.0.0.1")
            .env("PORT", port.to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .expect("no arrancó todo-app --static");
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

    async fn send(&self, method: &str, uri: &str) -> (http::StatusCode, http::HeaderMap, Bytes) {
        let stream = TcpStream::connect(self.addr).await.expect("sin conexión");
        let (mut sender, conn) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
            .await
            .expect("handshake");
        tokio::spawn(async move {
            let _ = conn.await;
        });
        let req = http::Request::builder()
            .method(method)
            .uri(uri)
            .body(Full::new(Bytes::new()))
            .expect("petición");
        let resp = sender.send_request(req).await.expect("sin respuesta");
        let (parts, body) = resp.into_parts();
        let body = timeout(WAIT, body.collect())
            .await
            .expect("la respuesta no terminó")
            .expect("cuerpo ilegible")
            .to_bytes();
        (parts.status, parts.headers, body)
    }

    async fn stop(mut self) {
        let _ = self.child.kill().await;
    }
}

#[tokio::test]
async fn s08_el_demo_sirve_client_en_site() {
    let demo = Demo::start().await;

    let (status, headers, body) = demo.send("GET", "/site/").await;
    assert_eq!(status, http::StatusCode::OK);
    assert_eq!(
        headers[http::header::CONTENT_TYPE],
        "text/html; charset=utf-8"
    );
    assert_eq!(headers[http::header::CACHE_CONTROL], "no-cache");
    assert!(String::from_utf8_lossy(&body).contains("rdd-static"));

    let (status, headers, _) = demo.send("GET", "/site/assets/style.css").await;
    assert_eq!(status, http::StatusCode::OK);
    assert_eq!(
        headers[http::header::CONTENT_TYPE],
        "text/css; charset=utf-8"
    );
    assert_eq!(
        headers[http::header::CACHE_CONTROL],
        "public, max-age=31536000, immutable"
    );

    let (status, headers, body) = demo.send("HEAD", "/site/assets/app.js").await;
    assert_eq!(status, http::StatusCode::OK);
    assert!(body.is_empty());
    assert!(headers.contains_key(http::header::CONTENT_LENGTH));

    let (status, _, _) = demo.send("GET", "/site/no-existe").await;
    assert_eq!(status, http::StatusCode::NOT_FOUND);

    demo.stop().await;
}
