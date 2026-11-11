//! C-13 a C-16 (§4.5): el transporte WebSocket de `rdd-axum`.
//!
//! Estas pruebas viven aquí porque el cliente WS es una dev-dependency de la
//! suite (§4.2) y los casos solo aplican a plataformas que declaran
//! `supports_upgrades()` (RP-027). Usan HTTP real contra un puerto efímero.

use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use futures::{SinkExt, StreamExt};
use rdd_axum::AxumPlatform;
use rdd_core::http::{BoxFuture, HttpApp, Middleware, Platform, ShutdownSignal, fn_middleware};
use rdd_core::socket::{SocketConnection, SocketContext, SocketHandler, SocketMessage};
use rdd_core::{App, Error};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

const BODY_LIMIT: usize = 64;
const ROOM: &str = "sala-1";
const MESSAGE_LIMIT: usize = 1024 * 1024;
const OUTBOUND_QUEUE: usize = 1;
const DRAIN: Duration = Duration::from_secs(15);

#[derive(Clone, Debug)]
struct Session {
    email: String,
}

/// Middleware global de la fixture: identifica o rechaza el handshake (RF-198).
fn identity() -> impl Middleware {
    fn_middleware(|mut req, next| async move {
        if req
            .headers()
            .get("x-test-auth")
            .is_some_and(|value| value == "deny")
        {
            return Err(Error::Unauthorized);
        }
        let email = req
            .headers()
            .get("x-test-session")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        if let Some(email) = email {
            req.extensions_mut().insert(Session { email });
        }
        next.run(req).await
    })
}

/// Canal de la fixture: ecos, cierre normal y dos formas de forzar 1013.
struct Echo;

impl SocketHandler for Echo {
    fn authorize(&self, ctx: &SocketContext) -> rdd_core::Result<()> {
        if ctx.param("room") == Some("forbidden") {
            return Err(Error::Forbidden);
        }
        ctx.ext::<Session>().map(|_| ()).ok_or(Error::Unauthorized)
    }

    fn handle(
        &self,
        ctx: SocketContext,
        mut conn: Box<dyn SocketConnection>,
    ) -> BoxFuture<'static, rdd_core::Result<()>> {
        Box::pin(async move {
            let room = ctx.param("room").unwrap_or("?").to_owned();
            let session = ctx.ext::<Session>().map(|s| s.email).unwrap_or_default();
            conn.send(SocketMessage::Text(format!(
                "room={room} session={session}"
            )))
            .await?;

            while let Some(incoming) = conn.recv().await {
                match incoming? {
                    SocketMessage::Text(text) if text == "bye" => return Ok(()),
                    SocketMessage::Text(text) if text == "flood" => {
                        let chunk = Bytes::from(vec![0u8; 32 * 1024]);
                        for _ in 0..256 {
                            if conn
                                .send(SocketMessage::Binary(chunk.clone()))
                                .await
                                .is_err()
                            {
                                return Ok(());
                            }
                        }
                    }
                    SocketMessage::Text(text) => {
                        conn.send(SocketMessage::Text(text)).await?;
                    }
                    SocketMessage::Binary(bytes) => {
                        conn.send(SocketMessage::Binary(bytes)).await?;
                    }
                    _ => {}
                }
            }
            Ok(())
        })
    }
}

fn http_app() -> HttpApp {
    let mut built = App::new()
        .without_health()
        .body_limit(BODY_LIMIT)
        .middleware(identity())
        .socket("/ws/echo/:room", Echo)
        .build()
        .expect("la app de canales debe construirse");
    built.http.limits.socket_message_limit_bytes = MESSAGE_LIMIT;
    built.http.limits.socket_outbound_queue = OUTBOUND_QUEUE;
    built.http
}

type Client = WebSocketStream<MaybeTlsStream<TcpStream>>;

async fn connect(
    addr: std::net::SocketAddr,
    headers: &[(&str, &str)],
) -> (Client, http::StatusCode) {
    let mut request = format!("ws://{addr}/ws/echo/{ROOM}")
        .into_client_request()
        .expect("la petición WS debe ser válida");
    for (name, value) in headers {
        request.headers_mut().insert(
            http::header::HeaderName::from_bytes(name.as_bytes()).expect("nombre válido"),
            http::header::HeaderValue::from_str(value).expect("cabecera válida"),
        );
    }
    let (socket, response) = tokio_tungstenite::connect_async(request)
        .await
        .unwrap_or_else(|err| panic!("el handshake WS falló: {err}"));
    (socket, response.status())
}

async fn next(ws: &mut Client) -> Message {
    tokio::time::timeout(Duration::from_secs(10), ws.next())
        .await
        .unwrap_or_else(|_| panic!("el servidor no envió nada en 10s"))
        .unwrap_or_else(|| panic!("el canal se cerró sin mensaje"))
        .unwrap_or_else(|err| panic!("error de trama: {err}"))
}

async fn next_close(ws: &mut Client) -> u16 {
    for _ in 0..4096 {
        if let Message::Close(frame) = next(ws).await {
            return frame.map(|frame| u16::from(frame.code)).unwrap_or(1000);
        }
    }
    panic!("no llegó un cierre en los mensajes leídos");
}

fn start() -> (
    std::net::SocketAddr,
    ShutdownSignal,
    tokio::task::JoinHandle<()>,
) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("puerto efímero");
    listener
        .set_nonblocking(true)
        .expect("listener no bloqueante");
    let addr = listener.local_addr().expect("socket vinculado");
    let listener = TcpListener::from_std(listener).expect("listener de Tokio");
    let app = http_app();
    let shutdown = ShutdownSignal::new();
    let future = Arc::new(AxumPlatform::new())
        .serve(app, listener, shutdown.clone())
        .expect("la plataforma debe arrancar");
    let server = tokio::spawn(async move {
        if let Err(err) = future.await {
            panic!("la plataforma terminó con error: {err}");
        }
    });
    (addr, shutdown, server)
}

async fn stop(shutdown: ShutdownSignal, server: tokio::task::JoinHandle<()>) {
    shutdown.trigger();
    tokio::time::timeout(DRAIN, server)
        .await
        .expect("la plataforma no cerró dentro del plazo")
        .expect("la tarea del servidor entró en pánico");
}

#[tokio::test]
async fn c13_handshake_101_con_parametros_e_identidad() {
    let (addr, shutdown, server) = start();
    let (mut ws, status) = connect(addr, &[("x-test-session", "alice")]).await;
    assert_eq!(status, 101, "C-13: el handshake debe ser 101");

    match next(&mut ws).await {
        Message::Text(text) => {
            assert!(text.contains("room=sala-1"), "parámetro de ruta: {text}");
            assert!(
                text.contains("session=alice"),
                "identidad del global: {text}"
            );
        }
        other => panic!("C-13: esperaba el saludo del handler, llegó {other:?}"),
    }

    stop(shutdown, server).await;
}

#[tokio::test]
async fn c14_middleware_rechaza_sin_101() {
    let (addr, shutdown, server) = start();
    let mut request = format!("ws://{addr}/ws/echo/{ROOM}")
        .into_client_request()
        .expect("petición válida");
    request.headers_mut().insert(
        http::header::HeaderName::from_static("x-test-auth"),
        http::header::HeaderValue::from_static("deny"),
    );
    match tokio_tungstenite::connect_async(request).await {
        Ok((_, response)) => panic!(
            "C-14: el middleware debía rechazar y llegó {}",
            response.status()
        ),
        Err(tokio_tungstenite::tungstenite::Error::Http(response)) => {
            assert_eq!(response.status(), http::StatusCode::UNAUTHORIZED);
            assert_eq!(
                response
                    .headers()
                    .get("content-type")
                    .and_then(|value| value.to_str().ok()),
                Some("application/problem+json"),
                "C-14: el rechazo es Problem Details"
            );
        }
        Err(err) => panic!("C-14: error inesperado del handshake: {err}"),
    }

    stop(shutdown, server).await;
}

#[tokio::test]
async fn c15_eco_texto_binario_ping_y_cierre_1000() {
    let (addr, shutdown, server) = start();
    let (mut ws, _) = connect(addr, &[("x-test-session", "alice")]).await;
    let _welcome = next(&mut ws).await;

    ws.send(Message::Text("hola".into()))
        .await
        .expect("envío de texto");
    match next(&mut ws).await {
        Message::Text(text) => assert_eq!(text.as_str(), "hola"),
        other => panic!("C-15: eco de texto, llegó {other:?}"),
    }

    ws.send(Message::Binary(Bytes::from_static(&[1, 2, 3])))
        .await
        .expect("envío binario");
    match next(&mut ws).await {
        Message::Binary(bytes) => assert_eq!(bytes.as_ref(), &[1, 2, 3]),
        other => panic!("C-15: eco binario, llegó {other:?}"),
    }

    ws.send(Message::Ping(Bytes::from_static(&[9])))
        .await
        .expect("envío de ping");
    match next(&mut ws).await {
        Message::Pong(payload) => assert_eq!(payload.as_ref(), &[9]),
        other => panic!("C-15: ping/pong, llegó {other:?}"),
    }

    ws.send(Message::Text("bye".into()))
        .await
        .expect("envío de cierre");
    assert_eq!(next_close(&mut ws).await, 1000, "C-15: cierre normal");

    stop(shutdown, server).await;
}

#[tokio::test]
async fn c16_mensaje_demasiado_grande_cierra_1009() {
    let (addr, shutdown, server) = start();
    let (mut ws, _) = connect(addr, &[("x-test-session", "alice")]).await;
    let _welcome = next(&mut ws).await;

    futures::SinkExt::send(
        &mut ws,
        Message::Binary(Bytes::from(vec![0u8; MESSAGE_LIMIT + 1])),
    )
    .await
    .expect("envío del mensaje fuera de límite");

    assert_eq!(next_close(&mut ws).await, 1009, "C-16: límite de mensaje");
    stop(shutdown, server).await;
}

#[tokio::test]
async fn c16_cliente_que_no_drena_cierra_1013() {
    let (addr, shutdown, server) = start();
    let (mut ws, _) = connect(addr, &[("x-test-session", "alice")]).await;
    let _welcome = next(&mut ws).await;

    ws.send(Message::Text("flood".into()))
        .await
        .expect("envío de flood");

    // El cliente deja de leer: la cola de salida se llena y el supervisor cierra
    // con 1013 en cuanto el socket vuelve a drenar (RF-196).
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(next_close(&mut ws).await, 1013, "C-16: cliente lento");
    stop(shutdown, server).await;
}

#[tokio::test]
async fn al_apagar_el_canal_cierra_con_1001() {
    let (addr, shutdown, server) = start();
    let (mut ws, _) = connect(addr, &[("x-test-session", "alice")]).await;
    let _welcome = next(&mut ws).await;

    shutdown.trigger();
    assert_eq!(next_close(&mut ws).await, 1001, "RP-026: apagado ordenado");
    stop(shutdown, server).await;
}
