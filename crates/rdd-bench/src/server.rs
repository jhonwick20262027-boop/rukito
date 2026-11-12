//! Servidor local del arnés: tres rutas HTTP y un canal WebSocket de eco.
//!
//! Es un `HttpApp` real servido por `AxumPlatform`, no un mock: mide el costo
//! del enrutado, la cadena de middleware y la traducción de respuestas.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use http::Method;
use rdd_core::Result;
use rdd_core::app::App;
use rdd_core::http::{BoxHandler, HttpApp, Response, RouteDef, ShutdownSignal, StatusCode};
use rdd_core::socket::{SocketConnection, SocketContext, SocketHandler, SocketMessage};

/// Cuerpo del POST de eco: JSON pequeño, el tamaño típico de un DTO.
pub const ECHO_BODY: &[u8] = br#"{"title":"benchmark","done":false}"#;

pub struct Running {
    pub addr: SocketAddr,
    shutdown: ShutdownSignal,
    task: tokio::task::JoinHandle<rdd_core::error::PlatformResult<()>>,
}

impl Running {
    pub async fn stop(self) {
        self.shutdown.trigger();
        let _ = tokio::time::timeout(Duration::from_secs(10), self.task).await;
    }
}

pub async fn start() -> anyhow::Result<Running> {
    let mut http = App::new()
        .without_health()
        .socket("/ws/echo", Echo)
        .build()?
        .http;

    http.routes.push(RouteDef::new(
        Method::GET,
        "/bench/ping",
        json_handler(|_req| (StatusCode::OK, r#"{"pong":true}"#.to_owned())),
    ));
    http.routes.push(RouteDef::new(
        Method::GET,
        "/bench/items/:id",
        json_handler(|req| {
            let id = req
                .extensions()
                .get::<rdd_core::http::PathParams>()
                .and_then(|params| params.get("id").map(str::to_owned))
                .unwrap_or_default();
            (
                StatusCode::OK,
                format!(r#"{{"id":"{id}","title":"item","done":false}}"#),
            )
        }),
    ));
    http.routes.push(RouteDef::new(
        Method::POST,
        "/bench/echo",
        Arc::new(|req| {
            Box::pin(async move {
                let _ = req;
                let mut resp = Response::new(Bytes::from_static(ECHO_BODY));
                *resp.status_mut() = StatusCode::OK;
                resp.headers_mut().insert(
                    http::header::CONTENT_TYPE,
                    http::HeaderValue::from_static("application/json"),
                );
                resp
            }) as rdd_core::http::BoxFuture<'static, Response>
        }),
    ));

    serve(http).await
}

fn json_handler<F>(build: F) -> BoxHandler
where
    F: Fn(&http::Request<Bytes>) -> (StatusCode, String) + Send + Sync + 'static,
{
    Arc::new(move |req| {
        let (status, body) = build(&req);
        Box::pin(async move {
            let mut resp = Response::new(Bytes::from(body));
            *resp.status_mut() = status;
            resp.headers_mut().insert(
                http::header::CONTENT_TYPE,
                http::HeaderValue::from_static("application/json"),
            );
            resp
        }) as rdd_core::http::BoxFuture<'static, Response>
    })
}

async fn serve(http: HttpApp) -> anyhow::Result<Running> {
    use rdd_core::http::Platform;
    use tokio::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let shutdown = ShutdownSignal::new();
    let serving = Arc::new(rdd_axum::AxumPlatform::new())
        .serve(http, listener, shutdown.clone())
        .map_err(|err| anyhow::anyhow!("el servidor de benchmarks no arrancó: {err}"))?;
    let task = tokio::spawn(serving);
    Ok(Running {
        addr,
        shutdown,
        task,
    })
}

/// Canal de eco: devuelve cada trama de texto o binaria tal cual.
struct Echo;

impl SocketHandler for Echo {
    fn handle(
        &self,
        _ctx: SocketContext,
        mut conn: Box<dyn SocketConnection>,
    ) -> rdd_core::http::BoxFuture<'static, Result<()>> {
        Box::pin(async move {
            while let Some(message) = conn.recv().await {
                match message? {
                    SocketMessage::Text(text) => conn.send(SocketMessage::Text(text)).await?,
                    SocketMessage::Binary(data) => conn.send(SocketMessage::Binary(data)).await?,
                    SocketMessage::Close(_, _) => break,
                    SocketMessage::Ping(_) | SocketMessage::Pong(_) => {}
                }
            }
            Ok(())
        })
    }
}
