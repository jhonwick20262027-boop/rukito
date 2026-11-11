//! Transporte WebSocket de esta plataforma (§10.4, RP-025).
//!
//! Solo traduce: el handshake lo decide `HttpApp::negotiate` y el handler del
//! núcleo recibe un [`SocketConnection`]. Aquí viven la cola acotada, los
//! límites de RF-196 y los códigos de cierre de RF-199.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use axum::body::{Body, to_bytes};
use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::extract::{FromRequestParts, Request};
use axum::response::Response;
use futures::{SinkExt, StreamExt};
use rdd_core::Error;
use rdd_core::http::{
    BoxFuture, HttpApp, Limits, ShutdownSignal, invoke_guarded, normalize_path, with_extensions,
};
use rdd_core::socket::{Handshake, Negotiation, SocketConnection, SocketMessage};
use tokio::sync::mpsc;
use tokio::sync::mpsc::error::TrySendError;

/// Espera máxima para escribir un cierre antes de soltar el socket.
const CLOSE_TIMEOUT: Duration = Duration::from_secs(2);

/// Handler de una ruta registrada en `HttpApp::upgrades` (RP-025).
pub(crate) async fn serve_upgrade(
    app: Arc<HttpApp>,
    shutdown: ShutdownSignal,
    params: HashMap<String, String>,
    req: Request,
) -> Response {
    let limits = app.limits;
    let (mut parts, body) = req.into_parts();
    // La petición normal ya trae el `OnUpgrade` en las extensiones; extraer el
    // extractor aquí decide si de verdad es un upgrade.
    let ws = WebSocketUpgrade::from_request_parts(&mut parts, &())
        .await
        .ok();
    let path = normalize_path(parts.uri.path());

    let bytes = match to_bytes(body, limits.body_limit_bytes).await {
        Ok(bytes) => bytes,
        Err(_) => {
            let problem = rdd_core::Problem::payload_too_large(limits.body_limit_bytes, &path);
            return axum_response(rdd_core::http::problem_response(
                rdd_core::http::StatusCode::PAYLOAD_TOO_LARGE,
                &problem,
                None,
            ));
        }
    };
    let core_req = http::Request::from_parts(parts, bytes);

    let Some(ws) = ws else {
        // Sin cabeceras de upgrade se comporta como una ruta HTTP normal: el
        // fallback decide 404 (RF-194).
        let resp = invoke_guarded(Arc::clone(&app.fallback), core_req, &path).await;
        return axum_response(resp);
    };

    let core_req = with_extensions(core_req, params.into_iter().collect(), None);
    match app.negotiate(core_req).await {
        Negotiation::Reject(resp) => axum_response(resp),
        Negotiation::Accept(handshake) => ws.on_upgrade(move |socket| async move {
            run(app, handshake, socket, shutdown).await;
        }),
    }
}

/// Convierte la respuesta del núcleo al tipo de Axum.
fn axum_response(resp: rdd_core::http::Response) -> Response {
    let (parts, body) = resp.into_parts();
    Response::from_parts(parts, Body::from(body))
}

/// Supervisa la conexión y el handler hasta que uno termina (RF-199).
async fn run(app: Arc<HttpApp>, handshake: Handshake, socket: WebSocket, shutdown: ShutdownSignal) {
    let limits = app.limits;
    let queue = limits.socket_outbound_queue.max(1);
    let (out_tx, out_rx) = mpsc::channel(queue);
    let (ctl_tx, ctl_rx) = mpsc::unbounded_channel();
    let (in_tx, in_rx) = mpsc::channel(queue);
    let closed = Arc::new(AtomicBool::new(false));
    let adapter = AxumSocket {
        inbound: in_rx,
        outbound: out_tx,
        control: ctl_tx.clone(),
        closed: Arc::clone(&closed),
    };

    tokio::spawn(supervise(
        socket,
        limits,
        out_rx,
        ctl_rx,
        in_tx,
        shutdown.clone(),
    ));

    let Handshake { context, handler } = handshake;
    let mut task = tokio::spawn(async move { handler.handle(context, Box::new(adapter)).await });

    // El `panic` de un handler queda aislado en su tarea (RF-073, RF-199).
    let (code, reason) = tokio::select! {
        joined = &mut task => match joined {
            Ok(Ok(())) => (1000, String::new()),
            Ok(Err(err)) => {
                tracing::warn!(error = %err, "el handler del canal terminó con error");
                (1011, "error del handler".to_owned())
            }
            Err(err) => {
                tracing::error!(error = %err, "el handler del canal entró en pánico");
                (1011, "error del handler".to_owned())
            }
        },
        _ = shutdown.wait() => {
            task.abort();
            (1001, "servidor apagándose".to_owned())
        }
    };

    // Si el supervisor ya cerró, estos canales están caídos y el aviso se
    // ignora: el cierre es idempotente (RF-199).
    let _ = ctl_tx.send(Control::Close(code, reason));
}

/// Dueño de la conexión mientras el handler vive (RF-195, RF-197).
struct AxumSocket {
    inbound: mpsc::Receiver<rdd_core::Result<SocketMessage>>,
    outbound: mpsc::Sender<Outbound>,
    control: mpsc::UnboundedSender<Control>,
    closed: Arc<AtomicBool>,
}

enum Outbound {
    Message(SocketMessage),
}

enum Control {
    Close(u16, String),
}

impl SocketConnection for AxumSocket {
    fn recv(&mut self) -> BoxFuture<'_, Option<rdd_core::Result<SocketMessage>>> {
        Box::pin(async move { self.inbound.recv().await })
    }

    fn send(&mut self, msg: SocketMessage) -> BoxFuture<'_, rdd_core::Result<()>> {
        Box::pin(async move {
            if self.closed.load(Ordering::SeqCst) {
                return Err(socket_error("el canal ya está cerrado"));
            }
            match self.outbound.try_send(Outbound::Message(msg)) {
                Ok(()) => Ok(()),
                Err(TrySendError::Closed(_)) => {
                    self.closed.store(true, Ordering::SeqCst);
                    Err(socket_error("el canal ya no está disponible"))
                }
                Err(TrySendError::Full(out)) => {
                    // Una vuelta al runtime deja drenar al supervisor; si la
                    // cola sigue llena, el cliente no drena (RF-196).
                    tokio::task::yield_now().await;
                    match self.outbound.try_send(out) {
                        Ok(()) => Ok(()),
                        Err(_) => {
                            self.closed.store(true, Ordering::SeqCst);
                            let _ = self
                                .control
                                .send(Control::Close(1013, "cliente lento".to_owned()));
                            Err(socket_error("el cliente no drena la cola de salida"))
                        }
                    }
                }
            }
        })
    }

    fn close(self: Box<Self>, code: u16, reason: String) -> BoxFuture<'static, ()> {
        Box::pin(async move {
            self.closed.store(true, Ordering::SeqCst);
            let _ = self.control.send(Control::Close(code, reason));
        })
    }
}

fn socket_error(message: &'static str) -> Error {
    Error::internal(anyhow::anyhow!(message))
}

/// Bucle único de la conexión: lee, escribe, atiende cierres y apagado.
async fn supervise(
    socket: WebSocket,
    limits: Limits,
    mut outbound: mpsc::Receiver<Outbound>,
    mut control: mpsc::UnboundedReceiver<Control>,
    inbound: mpsc::Sender<rdd_core::Result<SocketMessage>>,
    shutdown: ShutdownSignal,
) {
    let (mut sink, mut stream) = socket.split();
    // Al terminar el handler, el adaptador se suelta y su extremo de la cola de
    // salida se cierra. Eso no es motivo para cortar la conexión: el cierre
    // llega por el canal de control, que se atiende siempre (RF-199).
    let mut outbound_open = true;
    let mut inbound_open = true;

    loop {
        tokio::select! {
            ctl = control.recv() => match ctl {
                Some(Control::Close(code, reason)) => {
                    send_close(&mut sink, code, &reason).await;
                    break;
                }
                // Sin dueño del canal: se cierra como una terminación normal.
                None => {
                    send_close(&mut sink, 1000, "").await;
                    break;
                }
            },
            _ = shutdown.wait() => {
                send_close(&mut sink, 1001, "servidor apagándose").await;
                break;
            },
            out = outbound.recv(), if outbound_open => match out {
                Some(Outbound::Message(msg)) => {
                    if sink.send(to_axum(msg)).await.is_err() {
                        break;
                    }
                }
                None => outbound_open = false,
            },
            incoming = stream.next(), if inbound_open => match incoming {
                Some(Ok(msg)) => {
                    if exceeds(&msg, limits.socket_message_limit_bytes) {
                        // RF-196: no se entrega al handler.
                        send_close(&mut sink, 1009, "mensaje demasiado grande").await;
                        break;
                    }
                    let peer_close = match &msg {
                        Message::Close(frame) => Some((
                            frame.as_ref().map(|f| f.code).unwrap_or(1000),
                            frame.as_ref().map(|f| f.reason.to_string()).unwrap_or_default(),
                        )),
                        _ => None,
                    };
                    let delivered = match from_axum(msg) {
                        Some(msg) => inbound.send(Ok(msg)).await.is_ok(),
                        None => true,
                    };
                    if !delivered {
                        // El handler ya terminó; el resto se descarta hasta que
                        // llegue el cierre por el canal de control.
                        inbound_open = false;
                    }
                    if let Some((code, reason)) = peer_close {
                        send_close(&mut sink, code, &reason).await;
                        break;
                    }
                }
                Some(Err(err)) => {
                    tracing::debug!(error = %err, "el canal de lectura falló");
                    break;
                }
                None => break,
            },
        }
    }
}

/// Escribe un cierre con un plazo corto; si el socket no drena, se suelta.
async fn send_close(
    sink: &mut futures::stream::SplitSink<WebSocket, Message>,
    code: u16,
    reason: &str,
) {
    let frame = CloseFrame {
        code,
        reason: reason.to_owned().into(),
    };
    let _ = tokio::time::timeout(CLOSE_TIMEOUT, sink.send(Message::Close(Some(frame)))).await;
}

fn exceeds(msg: &Message, limit: usize) -> bool {
    match msg {
        Message::Text(text) => text.len() > limit,
        Message::Binary(bytes) => bytes.len() > limit,
        _ => false,
    }
}

fn to_axum(msg: SocketMessage) -> Message {
    match msg {
        SocketMessage::Text(text) => Message::Text(text.into()),
        SocketMessage::Binary(bytes) => Message::Binary(bytes),
        SocketMessage::Ping(bytes) => Message::Ping(bytes),
        SocketMessage::Pong(bytes) => Message::Pong(bytes),
        SocketMessage::Close(code, reason) => Message::Close(Some(CloseFrame {
            code: code.unwrap_or(1000),
            reason: reason.into(),
        })),
    }
}

fn from_axum(msg: Message) -> Option<SocketMessage> {
    match msg {
        Message::Text(text) => Some(SocketMessage::Text(text.to_string())),
        Message::Binary(bytes) => Some(SocketMessage::Binary(bytes)),
        Message::Ping(bytes) => Some(SocketMessage::Ping(bytes)),
        Message::Pong(bytes) => Some(SocketMessage::Pong(bytes)),
        Message::Close(frame) => Some(match frame {
            Some(frame) => SocketMessage::Close(Some(frame.code), frame.reason.to_string()),
            None => SocketMessage::Close(None, String::new()),
        }),
    }
}
