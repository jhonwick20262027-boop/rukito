//! Transporte stdio: una línea JSON por mensaje (§10.7, RF-214 a RF-219).
//!
//! `stdout` es exclusivamente protocolo; cualquier log va a `stderr` (RF-219).
//! El bucle de lectura nunca espera a una tool: cada `tools/call` corre en su
//! propia tarea para que `ping`, `tools/list` y la cancelación sigan vivos
//! (RF-217).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rdd_core::Result;
use serde_json::Value;
use tokio::io::{
    AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader, BufWriter,
};
use tokio::sync::{Semaphore, mpsc};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use crate::McpServer;
use crate::jsonrpc::{self, INTERNAL_ERROR, INVALID_REQUEST, METHOD_NOT_FOUND, PARSE_ERROR};
use crate::protocol::{self, CallRequest};
use crate::tool::ToolContext;

/// Tamaño máximo de una línea de entrada (RF-214).
pub const MAX_LINE_BYTES: usize = 8 * 1024 * 1024;

/// `tools/call` simultáneas; por encima se responde `-32603` sin ejecutar
/// (RF-217).
const MAX_IN_FLIGHT: usize = 64;

/// Plazo para cancelar y vaciar al cerrar (RF-215).
const SHUTDOWN_GRACE: Duration = Duration::from_secs(2);

/// Proceso real: lee `stdin` y escribe `stdout`.
pub async fn serve_stdio(server: McpServer) -> Result<()> {
    serve_io(server, tokio::io::stdin(), tokio::io::stdout()).await
}

/// Mismo bucle sobre cualquier par lector/escritor; lo usan las pruebas (RF-218).
pub async fn serve_io<R, W>(server: McpServer, reader: R, writer: W) -> Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin + Send + 'static,
{
    let (out, rx) = mpsc::unbounded_channel::<Value>();
    let (broken_tx, mut broken_rx) = mpsc::channel::<()>(1);
    let mut writer_task = tokio::spawn(write_loop(writer, rx, broken_tx));

    let mut connection = Connection::new(Arc::new(server), out.clone());
    let mut reader = BufReader::new(reader);

    loop {
        let outcome = tokio::select! {
            read = read_line_limited(&mut reader, MAX_LINE_BYTES) => match read {
                Ok(outcome) => outcome,
                Err(err) => {
                    tracing::debug!(error = %err, "el lector de stdio falló");
                    break;
                }
            },
            _ = broken_rx.recv() => break,
        };

        match outcome {
            ReadOutcome::Eof => break,
            ReadOutcome::Oversized => connection.send(jsonrpc::error(
                Value::Null,
                PARSE_ERROR,
                "línea demasiado grande",
            )),
            ReadOutcome::Line(bytes) => connection.handle_line(&bytes),
        }
    }

    connection.shutdown().await;
    drop(out);

    if tokio::time::timeout(SHUTDOWN_GRACE, &mut writer_task)
        .await
        .is_err()
    {
        writer_task.abort();
    }
    Ok(())
}

/// Estado de una conexión stdio.
struct Connection {
    server: Arc<McpServer>,
    out: mpsc::UnboundedSender<Value>,
    in_flight: Arc<Mutex<HashMap<String, CancellationToken>>>,
    slots: Arc<Semaphore>,
    tasks: JoinSet<()>,
    initialized: bool,
}

impl Connection {
    fn new(server: Arc<McpServer>, out: mpsc::UnboundedSender<Value>) -> Self {
        Self {
            server,
            out,
            in_flight: Arc::new(Mutex::new(HashMap::new())),
            slots: Arc::new(Semaphore::new(MAX_IN_FLIGHT)),
            tasks: JoinSet::new(),
            initialized: false,
        }
    }

    fn send(&self, value: Value) {
        let _ = self.out.send(value);
    }

    fn handle_line(&mut self, bytes: &[u8]) {
        let text = match std::str::from_utf8(bytes) {
            Ok(text) if !text.trim().is_empty() => text.trim(),
            Ok(_) => return,
            Err(_) => {
                self.send(jsonrpc::error(
                    Value::Null,
                    PARSE_ERROR,
                    "la línea no es UTF-8",
                ));
                return;
            }
        };

        let value: Value = match serde_json::from_str(text) {
            Ok(value) => value,
            Err(_) => {
                self.send(jsonrpc::error(Value::Null, PARSE_ERROR, "JSON inválido"));
                return;
            }
        };

        match jsonrpc::decode(&value) {
            Ok(request) => self.handle(request),
            Err(err) => self.send(jsonrpc::error(err.id, err.code, &err.message)),
        }
    }

    fn handle(&mut self, request: jsonrpc::Request) {
        match request.method.as_str() {
            "initialize" => self.initialize(request),
            "ping" => {
                if let Some(id) = request.id {
                    self.send(jsonrpc::response(id, serde_json::json!({})));
                }
            }
            "tools/list" => {
                let Some(id) = request.id else { return };
                if !self.initialized {
                    self.send(jsonrpc::error(
                        id,
                        INVALID_REQUEST,
                        "falta `initialize` antes de `tools/list`",
                    ));
                    return;
                }
                self.send(jsonrpc::response(id, protocol::tools_list(&self.server)));
            }
            "tools/call" => self.call(request),
            "notifications/initialized" => {
                // El handshake ya quedó cerrado al responder `initialize`; con
                // `id` sería un request a un método que no existe (RF-206).
                if let Some(id) = request.id {
                    self.send(jsonrpc::error(
                        id,
                        METHOD_NOT_FOUND,
                        "`notifications/initialized` no es un método de petición",
                    ));
                }
            }
            "notifications/cancelled" => {
                if let Some(id) = request.id {
                    self.send(jsonrpc::error(
                        id,
                        METHOD_NOT_FOUND,
                        "`notifications/cancelled` no es un método de petición",
                    ));
                    return;
                }
                self.cancel(&request.params);
            }
            other => {
                if let Some(id) = request.id {
                    self.send(jsonrpc::error(
                        id,
                        METHOD_NOT_FOUND,
                        &format!("método no encontrado: {other}"),
                    ));
                }
            }
        }
    }

    fn initialize(&mut self, request: jsonrpc::Request) {
        let Some(id) = request.id else { return };
        if self.initialized {
            self.send(jsonrpc::error(
                id,
                INVALID_REQUEST,
                "`initialize` ya se procesó",
            ));
            return;
        }
        match protocol::initialize(&self.server, Some(&id), &request.params) {
            Ok(result) => {
                self.send(jsonrpc::response(id, result));
                self.initialized = true;
            }
            Err(err) => self.send(jsonrpc::error(err.id, err.code, &err.message)),
        }
    }

    fn call(&mut self, request: jsonrpc::Request) {
        let Some(id) = request.id else { return };
        if !self.initialized {
            self.send(jsonrpc::error(
                id,
                INVALID_REQUEST,
                "falta `initialize` antes de `tools/call`",
            ));
            return;
        }

        let call = match protocol::parse_call(&self.server, Some(&id), &request.params) {
            Ok(call) => call,
            Err(err) => {
                self.send(jsonrpc::error(err.id, err.code, &err.message));
                return;
            }
        };

        let permit = match Arc::clone(&self.slots).try_acquire_owned() {
            Ok(permit) => permit,
            Err(_) => {
                self.send(jsonrpc::error(
                    id,
                    INTERNAL_ERROR,
                    "demasiadas tools en vuelo",
                ));
                return;
            }
        };

        let key = serde_json::to_string(&id).unwrap_or_default();
        let cancel = CancellationToken::new();
        if let Ok(mut map) = self.in_flight.lock() {
            map.insert(key.clone(), cancel.clone());
        }

        let CallRequest {
            tool,
            args,
            progress_token,
        } = call;
        let out = self.out.clone();
        let in_flight = Arc::clone(&self.in_flight);

        self.tasks.spawn(async move {
            let ctx = ToolContext::new(progress_token, cancel.clone(), out.clone());
            let mut inner = tokio::spawn(async move { tool.call(args, ctx).await });

            // RF-212: al cancelarse no se responde; se aborta la tool.
            let outcome = tokio::select! {
                joined = &mut inner => Some(joined),
                _ = cancel.cancelled() => {
                    inner.abort();
                    None
                }
            };

            if let Ok(mut map) = in_flight.lock() {
                map.remove(&key);
            }
            drop(permit);

            match outcome {
                None => {}
                Some(Ok(Ok(result))) => {
                    let _ = out.send(jsonrpc::response(id, protocol::tool_ok(result)));
                }
                Some(Ok(Err(error))) => {
                    let _ = out.send(jsonrpc::response(id, protocol::tool_error(&error)));
                }
                Some(Err(join)) => {
                    // RF-210: el panic de la tool no tumba el proceso.
                    tracing::error!(error = %join, "la tool terminó sin respuesta");
                    let _ = out.send(jsonrpc::error(
                        id,
                        INTERNAL_ERROR,
                        "la tool entró en pánico",
                    ));
                }
            }
        });
    }

    fn cancel(&self, params: &Value) {
        let Some(request_id) = params
            .get("requestId")
            .filter(|value| value.is_string() || value.is_number())
        else {
            return;
        };
        let key = serde_json::to_string(request_id).unwrap_or_default();
        if let Ok(map) = self.in_flight.lock()
            && let Some(token) = map.get(&key)
        {
            token.cancel();
        }
    }

    /// RF-215: cancela lo pendiente, drena y aborta lo que no termine a tiempo.
    async fn shutdown(mut self) {
        let tokens: Vec<CancellationToken> = self
            .in_flight
            .lock()
            .map(|map| map.values().cloned().collect())
            .unwrap_or_default();
        for token in tokens {
            token.cancel();
        }

        let drained = tokio::time::timeout(SHUTDOWN_GRACE, async {
            while self.tasks.join_next().await.is_some() {}
        })
        .await;
        if drained.is_err() {
            self.tasks.abort_all();
            while self.tasks.join_next().await.is_some() {}
        }
    }
}

/// Serializa las salidas en una sola tarea: las líneas nunca se intercalan
/// (RF-216).
async fn write_loop<W>(
    writer: W,
    mut rx: mpsc::UnboundedReceiver<Value>,
    broken: mpsc::Sender<()>,
) -> std::io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    let mut writer = BufWriter::new(writer);
    while let Some(value) = rx.recv().await {
        let Ok(line) = serde_json::to_string(&value) else {
            continue;
        };
        let failed = writer.write_all(line.as_bytes()).await.is_err()
            || writer.write_all(b"\n").await.is_err()
            || writer.flush().await.is_err();
        if failed {
            // RF-215: el cliente cerró la tubería; se termina con éxito.
            let _ = broken.try_send(());
            return Ok(());
        }
    }
    let _ = writer.flush().await;
    Ok(())
}

enum ReadOutcome {
    Line(Vec<u8>),
    Oversized,
    Eof,
}

/// Lee hasta `\n` sin crecer por encima de `max` (RF-214).
async fn read_line_limited<R>(reader: &mut R, max: usize) -> std::io::Result<ReadOutcome>
where
    R: AsyncBufRead + Unpin,
{
    let mut buffer = Vec::new();
    let mut oversized = false;

    loop {
        let (consumed, newline) = {
            let available = reader.fill_buf().await?;
            if available.is_empty() {
                if buffer.is_empty() && !oversized {
                    return Ok(ReadOutcome::Eof);
                }
                if oversized {
                    return Ok(ReadOutcome::Oversized);
                }
                trim_cr(&mut buffer);
                return Ok(ReadOutcome::Line(buffer));
            }

            match available.iter().position(|&byte| byte == b'\n') {
                Some(position) => {
                    if !oversized {
                        buffer.extend_from_slice(&available[..position]);
                    }
                    (position + 1, true)
                }
                None => {
                    let length = available.len();
                    if !oversized {
                        buffer.extend_from_slice(available);
                        if buffer.len() > max {
                            buffer.clear();
                            oversized = true;
                        }
                    }
                    (length, false)
                }
            }
        };
        reader.consume(consumed);

        if newline {
            if oversized || buffer.len() > max {
                return Ok(ReadOutcome::Oversized);
            }
            trim_cr(&mut buffer);
            return Ok(ReadOutcome::Line(buffer));
        }
    }
}

fn trim_cr(buffer: &mut Vec<u8>) {
    if buffer.last() == Some(&b'\r') {
        buffer.pop();
    }
}
