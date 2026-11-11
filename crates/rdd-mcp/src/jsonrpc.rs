//! Validación y construcción de mensajes JSON-RPC 2.0 (§10.6, RF-207).
//!
//! Solo se decodifican peticiones y notificaciones: el servidor nunca inicia una
//! petición al cliente (RF-213), así que una respuesta entrante es un mensaje
//! inválido.

use serde_json::{Value, json};

/// La línea no es JSON (RF-207).
pub(crate) const PARSE_ERROR: i64 = -32700;
/// El objeto no cumple JSON-RPC: falta `jsonrpc`, `method` o el `id` no es válido.
pub(crate) const INVALID_REQUEST: i64 = -32600;
/// El método no está entre los soportados (RF-206).
pub(crate) const METHOD_NOT_FOUND: i64 = -32601;
/// Los parámetros del método no tienen la forma exigida (RF-209).
pub(crate) const INVALID_PARAMS: i64 = -32602;
/// Fallo interno, incluido el `panic` de una tool (RF-210).
pub(crate) const INTERNAL_ERROR: i64 = -32603;

/// Petición ya validada. `id == None` es una notificación (RF-207).
pub(crate) struct Request {
    pub id: Option<Value>,
    pub method: String,
    pub params: Value,
}

/// Error que se responde con el `id` de la petición (o `null` si no se pudo leer).
#[derive(Debug)]
pub(crate) struct RpcError {
    pub id: Value,
    pub code: i64,
    pub message: String,
}

impl RpcError {
    pub(crate) fn new(id: Value, code: i64, message: impl Into<String>) -> Self {
        Self {
            id,
            code,
            message: message.into(),
        }
    }

    pub(crate) fn invalid_request(id: Option<&Value>, message: impl Into<String>) -> Self {
        Self::new(id.cloned().unwrap_or(Value::Null), INVALID_REQUEST, message)
    }

    pub(crate) fn invalid_params(id: Option<&Value>, message: impl Into<String>) -> Self {
        Self::new(id.cloned().unwrap_or(Value::Null), INVALID_PARAMS, message)
    }
}

/// Decodifica un mensaje JSON-RPC ya parseado.
///
/// Un `id` ausente es una notificación; un `id` explícito `null` es una petición
/// con respuesta de `id: null`, como distingue `Option<Value>`.
pub(crate) fn decode(value: &Value) -> Result<Request, RpcError> {
    let Some(object) = value.as_object() else {
        return Err(RpcError::new(
            Value::Null,
            INVALID_REQUEST,
            "el mensaje debe ser un objeto JSON",
        ));
    };

    let id = match object.get("id") {
        None => None,
        Some(Value::Null) => Some(Value::Null),
        Some(id @ (Value::String(_) | Value::Number(_))) => Some(id.clone()),
        Some(_) => {
            return Err(RpcError::new(
                Value::Null,
                INVALID_REQUEST,
                "`id` debe ser una cadena, un número o null",
            ));
        }
    };

    if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Err(RpcError::invalid_request(
            id.as_ref(),
            "`jsonrpc` debe ser \"2.0\"",
        ));
    }

    let Some(method) = object.get("method").and_then(Value::as_str) else {
        return Err(RpcError::invalid_request(id.as_ref(), "falta `method`"));
    };

    let params = match object.get("params") {
        None => json!({}),
        Some(params @ Value::Object(_)) => params.clone(),
        Some(_) => {
            return Err(RpcError::invalid_params(
                id.as_ref(),
                "`params` debe ser un objeto",
            ));
        }
    };

    Ok(Request {
        id,
        method: method.to_owned(),
        params,
    })
}

/// Respuesta correcta con el `id` original intacto (RF-207).
pub(crate) fn response(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

/// Respuesta de error JSON-RPC.
pub(crate) fn error(id: Value, code: i64, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message }
    })
}

/// Notificación del servidor; nunca lleva `id` (RF-213).
pub(crate) fn notification(method: &str, params: Value) -> Value {
    json!({ "jsonrpc": "2.0", "method": method, "params": params })
}
