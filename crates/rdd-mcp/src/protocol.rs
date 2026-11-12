//! Semántica de los métodos MCP (§10.6, RF-205 a RF-209).
//!
//! Aquí no hay I/O: las funciones reciben parámetros ya decodificados y
//! devuelven el `result` o el error que el transporte escribe por stdio.

use std::sync::Arc;

use rdd_core::Error;
use serde_json::{Value, json};

use crate::McpServer;
use crate::jsonrpc::RpcError;
use crate::tool::{CallToolResult, Tool};

/// Versiones aceptadas, de la más reciente a la más antigua (RF-205).
pub(crate) const SUPPORTED_VERSIONS: &[&str] =
    &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

/// Versiones servidas por HTTP (RF-225, ADR-24).
///
/// Fuera quedan `2025-03-26`, que permitía batching, y `2024-11-05`, que usa el
/// transporte SSE heredado; ambas siguen disponibles por stdio.
pub(crate) const HTTP_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18"];

/// `initialize`: negocia la versión y publica capacidades y `serverInfo`.
pub(crate) fn initialize(
    server: &McpServer,
    id: Option<&Value>,
    params: &Value,
) -> Result<Value, RpcError> {
    initialize_with(server, id, params, SUPPORTED_VERSIONS)
}

/// `initialize` limitado a un subconjunto de versiones (RF-225).
pub(crate) fn initialize_with(
    server: &McpServer,
    id: Option<&Value>,
    params: &Value,
    versions: &[&str],
) -> Result<Value, RpcError> {
    let requested = params
        .get("protocolVersion")
        .and_then(Value::as_str)
        .ok_or_else(|| RpcError::invalid_params(id, "falta `protocolVersion`"))?;
    let version = if versions.contains(&requested) {
        requested
    } else {
        versions[0]
    };

    Ok(json!({
        "protocolVersion": version,
        "capabilities": { "tools": { "listChanged": false } },
        "serverInfo": { "name": server.name(), "version": server.version() }
    }))
}

/// `tools/list`: registro fijo en orden de registro (RF-208).
pub(crate) fn tools_list(server: &McpServer) -> Value {
    let tools: Vec<Value> = server
        .tools()
        .iter()
        .map(|tool| {
            json!({
                "name": tool.name(),
                "description": tool.description(),
                "inputSchema": tool.input_schema()
            })
        })
        .collect();
    json!({ "tools": tools })
}

/// `tools/call` ya validado: la tool, sus argumentos y el token de progreso.
pub(crate) struct CallRequest {
    pub tool: Arc<dyn Tool>,
    pub args: Value,
    pub progress_token: Option<Value>,
}

/// Resuelve la tool y valida `arguments` (RF-209).
pub(crate) fn parse_call(
    server: &McpServer,
    id: Option<&Value>,
    params: &Value,
) -> Result<CallRequest, RpcError> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| RpcError::invalid_params(id, "falta `name`"))?;
    let tool = server
        .tool_by_name(name)
        .ok_or_else(|| RpcError::invalid_params(id, format!("no existe la tool `{name}`")))?;

    let args = match params.get("arguments") {
        None | Some(Value::Null) => json!({}),
        Some(args @ Value::Object(_)) => args.clone(),
        Some(_) => {
            return Err(RpcError::invalid_params(
                id,
                "`arguments` debe ser un objeto",
            ));
        }
    };

    let progress_token = params
        .get("_meta")
        .and_then(|meta| meta.get("progressToken"))
        .filter(|token| token.is_string() || token.is_number())
        .cloned();

    Ok(CallRequest {
        tool,
        args,
        progress_token,
    })
}

/// Resultado exitoso de una tool.
pub(crate) fn tool_ok(result: CallToolResult) -> Value {
    serde_json::to_value(result).unwrap_or_else(|_| json!({ "content": [], "isError": true }))
}

/// Error de una tool: `isError: true` con el detalle público (RF-209, RF-210).
pub(crate) fn tool_error(error: &Error) -> Value {
    if error.is_internal() {
        // RF-210: el detalle va al log, al cliente solo el texto genérico.
        tracing::error!(error = %error, "la tool falló");
    }
    tool_ok(CallToolResult::error(error.client_detail()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_version_desconocida_cae_en_la_mas_reciente() {
        let server = McpServer::new("t", "0");
        let result = initialize(
            &server,
            Some(&json!(1)),
            &json!({ "protocolVersion": "1999-01-01" }),
        )
        .unwrap();
        assert_eq!(result["protocolVersion"], SUPPORTED_VERSIONS[0]);
    }

    #[test]
    fn initialize_exige_la_version() {
        let server = McpServer::new("t", "0");
        let error = initialize(&server, Some(&json!(1)), &json!({})).unwrap_err();
        assert_eq!(error.code, crate::jsonrpc::INVALID_PARAMS);
    }
}
