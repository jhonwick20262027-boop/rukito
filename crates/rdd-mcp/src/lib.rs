//! # rdd-mcp
//!
//! Servidor **MCP** (Model Context Protocol) por **stdio** (§10.6, §10.7). Es un
//! crate opcional de la fachada (feature `mcp`): implementa JSON-RPC 2.0 a mano y
//! expone las tools de un módulo, que son otro adaptador de entrada como el
//! controller (RA-019, ADR-22).
//!
//! ```no_run
//! use rdd_mcp::{CallToolResult, McpServer, Tool, ToolContext, serve_stdio};
//! use rdd_core::http::BoxFuture;
//! use serde_json::{Value, json};
//!
//! struct Echo;
//!
//! impl Tool for Echo {
//!     fn name(&self) -> &str { "echo" }
//!     fn description(&self) -> &str { "Devuelve el texto recibido." }
//!     fn input_schema(&self) -> Value {
//!         json!({
//!             "type": "object",
//!             "properties": { "text": { "type": "string" } },
//!             "required": ["text"]
//!         })
//!     }
//!     fn call(&self, args: Value, _ctx: ToolContext) -> BoxFuture<'static, rdd_core::Result<CallToolResult>> {
//!         Box::pin(async move {
//!             let text = args.get("text").and_then(Value::as_str).unwrap_or_default();
//!             Ok(CallToolResult::text(text))
//!         })
//!     }
//! }
//!
//! # async fn demo() -> rdd_core::Result<()> {
//! let server = McpServer::new("demo", "0.1.0").tool(Echo)?;
//! serve_stdio(server).await
//! # }
//! ```
//!
//! El protocolo es independiente del transporte (ADR-22): `serve_io` permite
//! probarlo sobre cualquier par lector/escritor, y `serve_stdio` lo conecta a las
//! tuberías del proceso.

mod http;
mod jsonrpc;
mod protocol;
mod tool;
mod transport;

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::anyhow;
use rdd_core::{Error, Result};
use serde_json::Value;

pub use http::{McpHttp, McpHttpConfig, McpHttpResponse, SseStream};
pub use rdd_core::http::Extensions;
pub use tool::{CallToolResult, Content, Tool, ToolContext};
pub use transport::{MAX_LINE_BYTES, serve_io, serve_io_with, serve_stdio, serve_stdio_with};

/// Servidor MCP con un registro **fijo** de tools (RF-208).
///
/// Se construye una vez al arrancar; `tools/list` nunca cambia y no se emite
/// `notifications/tools/list_changed`. Los duplicados y los nombres inválidos se
/// detectan al registrar, no al atender una petición.
pub struct McpServer {
    name: String,
    version: String,
    tools: Vec<Arc<dyn Tool>>,
    index: HashMap<String, Arc<dyn Tool>>,
}

impl McpServer {
    /// Servidor sin tools; `name` y `version` se publican en `initialize`.
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            tools: Vec::new(),
            index: HashMap::new(),
        }
    }

    /// Registra una tool. Falla si el nombre no cumple `[A-Za-z0-9_-]{1,64}`,
    /// si está repetido o si `inputSchema` no es un objeto con `type: object`
    /// (RF-208).
    pub fn tool<T: Tool>(mut self, tool: T) -> Result<Self> {
        let name = tool.name().to_owned();
        if !valid_tool_name(&name) {
            return Err(Error::internal(anyhow!(
                "nombre de tool inválido `{name}`: se exige [A-Za-z0-9_-]{{1,64}}"
            )));
        }
        if self.index.contains_key(&name) {
            return Err(Error::internal(anyhow!("tool duplicada `{name}`")));
        }
        let schema = tool.input_schema();
        if !schema.is_object() || schema.get("type").and_then(Value::as_str) != Some("object") {
            return Err(Error::internal(anyhow!(
                "`inputSchema` de `{name}` debe ser un objeto con \"type\": \"object\""
            )));
        }

        let tool: Arc<dyn Tool> = Arc::new(tool);
        self.index.insert(name, Arc::clone(&tool));
        self.tools.push(tool);
        Ok(self)
    }

    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    pub(crate) fn version(&self) -> &str {
        &self.version
    }

    pub(crate) fn tools(&self) -> &[Arc<dyn Tool>] {
        &self.tools
    }

    pub(crate) fn tool_by_name(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.index.get(name).cloned()
    }
}

fn valid_tool_name(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct Probe {
        name: &'static str,
        schema: Value,
    }

    impl Tool for Probe {
        fn name(&self) -> &str {
            self.name
        }
        fn description(&self) -> &str {
            "sonda"
        }
        fn input_schema(&self) -> Value {
            self.schema.clone()
        }
        fn call(
            &self,
            _args: Value,
            _ctx: ToolContext,
        ) -> rdd_core::http::BoxFuture<'static, Result<CallToolResult>> {
            Box::pin(async { Ok(CallToolResult::text("ok")) })
        }
    }

    fn probe(name: &'static str) -> Probe {
        Probe {
            name,
            schema: json!({ "type": "object", "properties": {} }),
        }
    }

    #[test]
    fn rechaza_nombres_invalidos_y_duplicados() {
        assert!(
            McpServer::new("t", "0")
                .tool(probe("dos palabras"))
                .is_err()
        );
        assert!(McpServer::new("t", "0").tool(probe("")).is_err());
        let server = McpServer::new("t", "0").tool(probe("echo")).unwrap();
        assert!(server.tool(probe("echo")).is_err());
    }

    #[test]
    fn exige_un_esquema_de_objeto() {
        let bad = Probe {
            name: "bad",
            schema: json!({ "type": "string" }),
        };
        assert!(McpServer::new("t", "0").tool(bad).is_err());
    }
}
