//! Binario de prueba para M-07: servidor MCP mínimo por stdio.
//!
//! La suite lo lanza como proceso real y comprueba que cada línea de `stdout`
//! es un JSON completo y que el proceso termina al cerrarse `stdin` (RF-214,
//! RF-215, RF-216, RF-219).

use rdd_core::http::BoxFuture;
use rdd_mcp::{CallToolResult, McpServer, Tool, ToolContext, serve_stdio};
use serde_json::{Value, json};

struct Echo;

impl Tool for Echo {
    fn name(&self) -> &str {
        "echo"
    }

    fn description(&self) -> &str {
        "Devuelve el argumento `text`."
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
            let text = args.get("text").and_then(Value::as_str).unwrap_or_default();
            Ok(CallToolResult::text(text))
        })
    }
}

#[tokio::main]
async fn main() -> rdd_core::Result<()> {
    let server = McpServer::new("rdd-mcp-echo", env!("CARGO_PKG_VERSION")).tool(Echo)?;
    serve_stdio(server).await
}
