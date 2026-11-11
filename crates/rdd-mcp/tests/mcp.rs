//! Suite MCP: casos M-01 a M-08 sobre `serve_io` y M-07 con el binario real
//! (§20, RF-220, RF-221).

use std::sync::Arc;
use std::time::Duration;

use rdd_core::Error;
use rdd_core::http::BoxFuture;
use rdd_mcp::{CallToolResult, McpServer, Tool, ToolContext, serve_io};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream, duplex};
use tokio::sync::Notify;
use tokio::task::JoinHandle;
use tokio::time::timeout;

const WAIT: Duration = Duration::from_secs(5);

/// Cliente de prueba: dos tuberías en memoria y el servidor en su tarea.
struct Client {
    tx: DuplexStream,
    rx: BufReader<DuplexStream>,
    server: JoinHandle<rdd_core::Result<()>>,
}

impl Client {
    fn start(server: McpServer) -> Self {
        let (tx, server_rx) = duplex(64 * 1024);
        let (server_tx, rx) = duplex(64 * 1024);
        let server = tokio::spawn(serve_io(server, server_rx, server_tx));
        Self {
            tx,
            rx: BufReader::new(rx),
            server,
        }
    }

    async fn send(&mut self, value: Value) {
        self.send_raw(&value.to_string()).await;
    }

    async fn send_raw(&mut self, text: &str) {
        self.tx.write_all(text.as_bytes()).await.unwrap();
        self.tx.write_all(b"\n").await.unwrap();
        self.tx.flush().await.unwrap();
    }

    async fn recv(&mut self) -> Value {
        let mut line = String::new();
        let read = timeout(WAIT, self.rx.read_line(&mut line))
            .await
            .expect("el servidor no respondió a tiempo")
            .expect("fallo leyendo stdout");
        assert!(read > 0, "el servidor cerró stdout");
        serde_json::from_str(&line).unwrap_or_else(|_| panic!("línea no JSON: {line:?}"))
    }

    async fn expect_silence(&mut self, wait: Duration) {
        let mut line = String::new();
        match timeout(wait, self.rx.read_line(&mut line)).await {
            Err(_) => {}
            Ok(Ok(0)) => panic!("el servidor cerró stdout"),
            Ok(Ok(_)) => panic!("llegó un mensaje inesperado: {line}"),
            Ok(Err(err)) => panic!("fallo leyendo stdout: {err}"),
        }
    }

    async fn handshake(&mut self) {
        self.send(json!({
            "jsonrpc": "2.0",
            "id": 0,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": { "name": "suite", "version": "1" }
            }
        }))
        .await;
        let response = self.recv().await;
        assert_eq!(response["id"], 0);
        assert!(
            response.get("error").is_none(),
            "initialize falló: {response}"
        );
        self.send(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))
            .await;
    }

    async fn close(self) {
        let Client { tx, rx, server } = self;
        drop(tx);
        drop(rx);
        let joined = timeout(WAIT, server)
            .await
            .expect("el servidor no terminó al cerrar stdin")
            .expect("la tarea del servidor entró en pánico");
        joined.expect("serve_io devolvió error");
    }
}

struct Echo;

impl Tool for Echo {
    fn name(&self) -> &str {
        "echo"
    }

    fn description(&self) -> &str {
        "Devuelve `text` o `{}` si no hay argumentos."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": { "text": { "type": "string" } }
        })
    }

    fn call(
        &self,
        args: Value,
        _ctx: ToolContext,
    ) -> BoxFuture<'static, rdd_core::Result<CallToolResult>> {
        Box::pin(async move {
            match args.get("text").and_then(Value::as_str) {
                Some(text) => Ok(CallToolResult::text(text)),
                None => Ok(CallToolResult::text("{}")),
            }
        })
    }
}

struct Probe {
    name: &'static str,
    description: &'static str,
}

impl Tool for Probe {
    fn name(&self) -> &str {
        self.name
    }

    fn description(&self) -> &str {
        self.description
    }

    fn input_schema(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }

    fn call(
        &self,
        _args: Value,
        _ctx: ToolContext,
    ) -> BoxFuture<'static, rdd_core::Result<CallToolResult>> {
        Box::pin(async { Ok(CallToolResult::text("ok")) })
    }
}

struct Failing;

impl Tool for Failing {
    fn name(&self) -> &str {
        "failing"
    }

    fn description(&self) -> &str {
        "Devuelve un error de dominio."
    }

    fn input_schema(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }

    fn call(
        &self,
        _args: Value,
        _ctx: ToolContext,
    ) -> BoxFuture<'static, rdd_core::Result<CallToolResult>> {
        Box::pin(async { Err(Error::bad_request("falta el nombre")) })
    }
}

struct Boom;

impl Tool for Boom {
    fn name(&self) -> &str {
        "boom"
    }

    fn description(&self) -> &str {
        "Entra en pánico."
    }

    fn input_schema(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }

    fn call(
        &self,
        _args: Value,
        _ctx: ToolContext,
    ) -> BoxFuture<'static, rdd_core::Result<CallToolResult>> {
        Box::pin(async { panic!("pánico de prueba") })
    }
}

struct Slow {
    started: Arc<Notify>,
}

impl Tool for Slow {
    fn name(&self) -> &str {
        "slow"
    }

    fn description(&self) -> &str {
        "Tarda un segundo; sirve para cancelar."
    }

    fn input_schema(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }

    fn call(
        &self,
        _args: Value,
        _ctx: ToolContext,
    ) -> BoxFuture<'static, rdd_core::Result<CallToolResult>> {
        let started = Arc::clone(&self.started);
        Box::pin(async move {
            started.notify_one();
            tokio::time::sleep(Duration::from_secs(1)).await;
            Ok(CallToolResult::text("terminó"))
        })
    }
}

struct Progress;

impl Tool for Progress {
    fn name(&self) -> &str {
        "progress"
    }

    fn description(&self) -> &str {
        "Emite dos notificaciones de progreso."
    }

    fn input_schema(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }

    fn call(
        &self,
        _args: Value,
        ctx: ToolContext,
    ) -> BoxFuture<'static, rdd_core::Result<CallToolResult>> {
        Box::pin(async move {
            ctx.progress(1.0, Some(2.0))?;
            ctx.progress(2.0, None)?;
            Ok(CallToolResult::text("ok"))
        })
    }
}

fn server_with(tool: impl Tool) -> McpServer {
    McpServer::new("pruebas", "9.9.9").tool(tool).unwrap()
}

fn valid_tool_name(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

#[tokio::test]
async fn m01_initialize_negocia_version_y_capacidades() {
    let mut client = Client::start(server_with(Echo));

    client
        .send(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "m01", "version": "1" }
            }
        }))
        .await;
    let response = client.recv().await;
    assert_eq!(response["id"], 1);
    assert_eq!(response["result"]["protocolVersion"], "2024-11-05");
    assert_eq!(
        response["result"]["capabilities"],
        json!({ "tools": { "listChanged": false } })
    );
    assert_eq!(
        response["result"]["serverInfo"],
        json!({ "name": "pruebas", "version": "9.9.9" })
    );

    // `notifications/initialized` no responde: la siguiente línea es el ping.
    client
        .send(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))
        .await;
    client
        .send(json!({ "jsonrpc": "2.0", "id": 2, "method": "ping" }))
        .await;
    let ping = client.recv().await;
    assert_eq!(ping["id"], 2);
    assert_eq!(ping["result"], json!({}));
    client.close().await;

    // Una versión desconocida cae en la más reciente y un segundo
    // `initialize` es `-32600`.
    let mut client = Client::start(server_with(Echo));
    client
        .send(json!({
            "jsonrpc": "2.0",
            "id": "v",
            "method": "initialize",
            "params": { "protocolVersion": "1999-01-01", "capabilities": {} }
        }))
        .await;
    let response = client.recv().await;
    assert_eq!(response["result"]["protocolVersion"], "2025-11-25");

    client
        .send(json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "initialize",
            "params": { "protocolVersion": "2025-06-18", "capabilities": {} }
        }))
        .await;
    let duplicate = client.recv().await;
    assert_eq!(duplicate["id"], 3);
    assert_eq!(duplicate["error"]["code"], -32600);
    client.close().await;
}

#[tokio::test]
async fn m02_ping_conserva_el_id_y_el_handshake_es_obligatorio() {
    let mut client = Client::start(server_with(Echo));

    client
        .send(json!({ "jsonrpc": "2.0", "id": 7, "method": "ping" }))
        .await;
    let numeric = client.recv().await;
    assert_eq!(numeric["id"], 7);
    assert_eq!(numeric["result"], json!({}));

    client
        .send(json!({ "jsonrpc": "2.0", "id": "siete", "method": "ping" }))
        .await;
    let string = client.recv().await;
    assert_eq!(string["id"], "siete");

    client
        .send(json!({ "jsonrpc": "2.0", "id": 5, "method": "tools/list" }))
        .await;
    let before = client.recv().await;
    assert_eq!(before["id"], 5);
    assert_eq!(before["error"]["code"], -32600);
    client.close().await;
}

#[tokio::test]
async fn m03_tools_list_devuelve_el_registro_en_orden() {
    let server = McpServer::new("pruebas", "9.9.9")
        .tool(Probe {
            name: "alpha",
            description: "primera",
        })
        .unwrap()
        .tool(Probe {
            name: "beta",
            description: "segunda",
        })
        .unwrap()
        .tool(Probe {
            name: "gamma",
            description: "tercera",
        })
        .unwrap();
    let mut client = Client::start(server);
    client.handshake().await;

    client
        .send(json!({ "jsonrpc": "2.0", "id": 4, "method": "tools/list" }))
        .await;
    let response = client.recv().await;
    assert_eq!(response["id"], 4);
    let tools = response["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 3);
    for (tool, expected) in tools.iter().zip(["alpha", "beta", "gamma"]) {
        assert_eq!(tool["name"], expected);
        assert!(valid_tool_name(expected));
        assert!(tool["description"].is_string());
        assert_eq!(tool["inputSchema"]["type"], "object");
    }
    client.close().await;
}

#[tokio::test]
async fn m04_tools_call_entrega_argumentos_y_texto() {
    let mut client = Client::start(server_with(Echo));
    client.handshake().await;

    client
        .send(json!({
            "jsonrpc": "2.0",
            "id": 6,
            "method": "tools/call",
            "params": { "name": "echo", "arguments": { "text": "hola" } }
        }))
        .await;
    let response = client.recv().await;
    assert_eq!(response["id"], 6);
    assert_eq!(response["result"]["isError"], false);
    assert_eq!(response["result"]["content"][0]["type"], "text");
    assert_eq!(response["result"]["content"][0]["text"], "hola");

    // Sin `arguments` equivale a `{}`.
    client
        .send(json!({
            "jsonrpc": "2.0",
            "id": 7,
            "method": "tools/call",
            "params": { "name": "echo" }
        }))
        .await;
    let response = client.recv().await;
    assert_eq!(response["result"]["content"][0]["text"], "{}");
    client.close().await;
}

#[tokio::test]
async fn m05_error_de_tool_y_panico_aislado() {
    let server = McpServer::new("pruebas", "9.9.9")
        .tool(Failing)
        .unwrap()
        .tool(Boom)
        .unwrap()
        .tool(Echo)
        .unwrap();
    let mut client = Client::start(server);
    client.handshake().await;

    client
        .send(json!({
            "jsonrpc": "2.0",
            "id": 10,
            "method": "tools/call",
            "params": { "name": "failing", "arguments": {} }
        }))
        .await;
    let failed = client.recv().await;
    assert_eq!(failed["id"], 10);
    assert!(failed.get("error").is_none());
    assert_eq!(failed["result"]["isError"], true);
    assert_eq!(failed["result"]["content"][0]["text"], "falta el nombre");

    client
        .send(json!({
            "jsonrpc": "2.0",
            "id": 11,
            "method": "tools/call",
            "params": { "name": "boom", "arguments": {} }
        }))
        .await;
    let panicked = client.recv().await;
    assert_eq!(panicked["id"], 11);
    assert!(panicked.get("result").is_none());
    assert_eq!(panicked["error"]["code"], -32603);

    // El servidor sigue atendiendo después del pánico.
    client
        .send(json!({
            "jsonrpc": "2.0",
            "id": 12,
            "method": "tools/call",
            "params": { "name": "echo", "arguments": { "text": "vivo" } }
        }))
        .await;
    let alive = client.recv().await;
    assert_eq!(alive["id"], 12);
    assert_eq!(alive["result"]["content"][0]["text"], "vivo");
    client.close().await;
}

#[tokio::test]
async fn m06_errores_jsonrpc_no_tumban_el_servidor() {
    let mut client = Client::start(server_with(Echo));

    client.send_raw("esto no es json").await;
    let parse_error = client.recv().await;
    assert_eq!(parse_error["id"], Value::Null);
    assert_eq!(parse_error["error"]["code"], -32700);

    client.send(json!({ "jsonrpc": "2.0", "id": 1 })).await;
    let invalid = client.recv().await;
    assert_eq!(invalid["id"], 1);
    assert_eq!(invalid["error"]["code"], -32600);

    // Una notificación desconocida se ignora: la siguiente línea es el ping.
    client
        .send(json!({ "jsonrpc": "2.0", "method": "resources/list" }))
        .await;
    client
        .send(json!({ "jsonrpc": "2.0", "id": 2, "method": "ping" }))
        .await;
    let ping = client.recv().await;
    assert_eq!(ping["id"], 2);

    client.handshake().await;

    client
        .send(json!({ "jsonrpc": "2.0", "id": 4, "method": "recursos/list" }))
        .await;
    let unknown = client.recv().await;
    assert_eq!(unknown["id"], 4);
    assert_eq!(unknown["error"]["code"], -32601);

    client
        .send(json!({
            "jsonrpc": "2.0",
            "id": 5,
            "method": "tools/call",
            "params": { "name": "no-existe", "arguments": {} }
        }))
        .await;
    let missing = client.recv().await;
    assert_eq!(missing["id"], 5);
    assert_eq!(missing["error"]["code"], -32602);
    assert!(
        missing["error"]["message"]
            .as_str()
            .unwrap()
            .contains("no-existe")
    );

    client
        .send(json!({
            "jsonrpc": "2.0",
            "id": 6,
            "method": "tools/call",
            "params": { "name": "echo", "arguments": [] }
        }))
        .await;
    let bad_args = client.recv().await;
    assert_eq!(bad_args["id"], 6);
    assert_eq!(bad_args["error"]["code"], -32602);

    client
        .send(json!({ "jsonrpc": "2.0", "id": 7, "method": "ping" }))
        .await;
    let alive = client.recv().await;
    assert_eq!(alive["id"], 7);
    client.close().await;
}

#[tokio::test]
async fn m07_binario_real_escribe_solo_protocolo() {
    use std::process::Stdio;
    use tokio::process::Command;

    let mut child = Command::new(env!("CARGO_BIN_EXE_rdd-mcp-echo"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .expect("no arrancó el binario");

    let mut stdin = child.stdin.take().expect("sin stdin");
    let stdout = child.stdout.take().expect("sin stdout");
    let mut lines = BufReader::new(stdout).lines();

    stdin
        .write_all(
            format!(
                "{}\n",
                json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "method": "initialize",
                    "params": {
                        "protocolVersion": "2025-06-18",
                        "capabilities": {},
                        "clientInfo": { "name": "m07", "version": "1" }
                    }
                })
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    stdin.flush().await.unwrap();

    let line = timeout(WAIT, lines.next_line())
        .await
        .expect("initialize no respondió")
        .unwrap()
        .expect("stdout cerrado");
    let response: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(response["id"], 1);
    assert_eq!(response["result"]["protocolVersion"], "2025-06-18");

    stdin
        .write_all(
            format!(
                "{}\n",
                json!({
                    "jsonrpc": "2.0",
                    "id": 2,
                    "method": "tools/call",
                    "params": { "name": "echo", "arguments": { "text": "proceso" } }
                })
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    stdin.flush().await.unwrap();

    let line = timeout(WAIT, lines.next_line())
        .await
        .expect("tools/call no respondió")
        .unwrap()
        .expect("stdout cerrado");
    let response: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(response["id"], 2);
    assert_eq!(response["result"]["content"][0]["text"], "proceso");

    // Cerrar `stdin` (EOF) termina el proceso con éxito y sin más líneas.
    drop(stdin);
    let mut extra = Vec::new();
    while let Ok(Ok(Some(line))) = timeout(WAIT, lines.next_line()).await {
        extra.push(line);
    }
    assert!(
        extra.is_empty(),
        "stdout llevó líneas inesperadas: {extra:?}"
    );

    let status = timeout(WAIT, child.wait())
        .await
        .expect("el proceso no terminó con EOF")
        .unwrap();
    assert!(status.success());
}

#[tokio::test]
async fn m08_cancelled_corta_la_tool_sin_responder() {
    let started = Arc::new(Notify::new());
    let mut client = Client::start(server_with(Slow {
        started: Arc::clone(&started),
    }));
    client.handshake().await;

    client
        .send(json!({
            "jsonrpc": "2.0",
            "id": 42,
            "method": "tools/call",
            "params": { "name": "slow", "arguments": {} }
        }))
        .await;
    timeout(WAIT, started.notified())
        .await
        .expect("la tool no arrancó");

    client
        .send(json!({
            "jsonrpc": "2.0",
            "method": "notifications/cancelled",
            "params": { "requestId": 42 }
        }))
        .await;
    client
        .send(json!({ "jsonrpc": "2.0", "id": 43, "method": "ping" }))
        .await;
    let ping = client.recv().await;
    assert_eq!(ping["id"], 43);

    // Si la cancelación fallara, la tool respondería al segundo.
    client.expect_silence(Duration::from_millis(1500)).await;
    client.close().await;
}

#[tokio::test]
async fn m08_progress_llega_antes_del_resultado() {
    let mut client = Client::start(server_with(Progress));
    client.handshake().await;

    client
        .send(json!({
            "jsonrpc": "2.0",
            "id": 8,
            "method": "tools/call",
            "params": {
                "name": "progress",
                "arguments": {},
                "_meta": { "progressToken": "tok-1" }
            }
        }))
        .await;

    let first = client.recv().await;
    assert_eq!(first["method"], "notifications/progress");
    assert_eq!(first["params"]["progressToken"], "tok-1");
    assert_eq!(first["params"]["progress"], 1.0);
    assert_eq!(first["params"]["total"], 2.0);

    let second = client.recv().await;
    assert_eq!(second["method"], "notifications/progress");
    assert_eq!(second["params"]["progress"], 2.0);
    assert!(second["params"].get("total").is_none());

    let result = client.recv().await;
    assert_eq!(result["id"], 8);
    assert_eq!(result["result"]["isError"], false);
    assert_eq!(result["result"]["content"][0]["text"], "ok");
    client.close().await;
}
