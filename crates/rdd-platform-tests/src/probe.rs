//! Las dos sondas de la suite (§4.5): HTTP real y [`HttpApp::dispatch`] (RP-021).
//!
//! Cada caso escribe su aserción una sola vez; la sonda decide si la petición
//! viaja por un socket o entra directo al núcleo.

use std::net::SocketAddr;
use std::sync::Arc;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper_util::rt::TokioIo;
use rdd_core::http::{HttpApp, Response, ShutdownSignal};
use tokio::net::TcpStream;

/// Petición sin cuerpo materializado: el caso la arma y la sonda decide cómo
/// convertirla (`Bytes` para el núcleo, `Full` para hyper).
pub(crate) type ProbeRequest = http::Request<Vec<u8>>;

/// Cómo llega una petición a lo que se está probando.
#[derive(Clone)]
pub(crate) enum Probe {
    /// Plataforma real escuchando en un puerto efímero (§4.5).
    Http {
        addr: SocketAddr,
        shutdown: ShutdownSignal,
    },
    /// Plataforma de referencia del núcleo, sin red (RP-014, RP-021).
    InProcess(Arc<HttpApp>),
}

impl Probe {
    /// Cierto si la sonda usa un socket de verdad (C-11 solo aplica ahí).
    pub(crate) fn is_http(&self) -> bool {
        matches!(self, Self::Http { .. })
    }

    /// Dirección de escucha, solo con sonda de red.
    pub(crate) fn addr(&self) -> SocketAddr {
        match self {
            Self::Http { addr, .. } => *addr,
            Self::InProcess(_) => panic!("la sonda en proceso no tiene dirección"),
        }
    }

    /// Señal de apagado de la plataforma bajo prueba.
    pub(crate) fn shutdown(&self) -> ShutdownSignal {
        match self {
            Self::Http { shutdown, .. } => shutdown.clone(),
            Self::InProcess(_) => panic!("la sonda en proceso no se apaga por señal"),
        }
    }

    /// Envía la petición y devuelve la respuesta con el cuerpo ya en memoria.
    ///
    /// Un fallo de transporte hace `panic`: en la suite, no poder hablarle al
    /// servidor es un fallo del caso, no una espera razonable. Una conexión por
    /// petición, para que ningún caso dependa del orden de un `Keep-Alive`.
    pub(crate) async fn send(&self, req: ProbeRequest) -> Response {
        match self {
            Self::Http { addr, .. } => send_over_http(*addr, req).await,
            Self::InProcess(app) => {
                let (parts, body) = req.into_parts();
                app.dispatch(http::Request::from_parts(parts, Bytes::from(body)))
                    .await
            }
        }
    }
}

/// Una conexión nueva, una petición, la respuesta completa.
async fn send_over_http(addr: SocketAddr, req: ProbeRequest) -> Response {
    let (mut sender, conn) = handshake(addr).await;
    tokio::spawn(async move {
        let _ = conn.await;
    });

    let (parts, body) = req.into_parts();
    let resp = sender
        .send_request(http::Request::from_parts(
            parts,
            Full::new(Bytes::from(body)),
        ))
        .await
        .unwrap_or_else(|err| panic!("la petición a {addr} no obtuvo respuesta: {err}"));
    collect(resp).await
}

/// Abre el socket y hace el handshake HTTP/1.1.
async fn handshake(
    addr: SocketAddr,
) -> (
    hyper::client::conn::http1::SendRequest<Full<Bytes>>,
    hyper::client::conn::http1::Connection<TokioIo<TcpStream>, Full<Bytes>>,
) {
    let io = TokioIo::new(
        TcpStream::connect(addr)
            .await
            .unwrap_or_else(|err| panic!("no se pudo conectar a {addr}: {err}")),
    );
    hyper::client::conn::http1::handshake(io)
        .await
        .unwrap_or_else(|err| panic!("falló el handshake con {addr}: {err}"))
}

/// Convierte la respuesta de hyper al tipo del núcleo (`Response<Bytes>`, RP-002).
async fn collect(resp: hyper::Response<hyper::body::Incoming>) -> Response {
    let (parts, body) = resp.into_parts();
    let collected = body
        .collect()
        .await
        .unwrap_or_else(|err| panic!("no se pudo leer el cuerpo de la respuesta: {err}"));
    http::Response::from_parts(parts, collected.to_bytes())
}

/// Cierto si `addr` ya no acepta conexiones nuevas (RP-009).
///
/// C-11 lo usa después de activar el apagado: con el listener cerrado el SO
/// rechaza el `connect`; si el backlog todavía lo acepta, el handshake HTTP no
/// puede completarse. Ambas situaciones cuentan como «rechazada».
pub(crate) async fn refuses_connections(addr: SocketAddr) -> bool {
    match TcpStream::connect(addr).await {
        Err(_) => true,
        Ok(stream) => {
            { hyper::client::conn::http1::handshake::<_, Full<Bytes>>(TokioIo::new(stream)) }
                .await
                .is_err()
        }
    }
}
