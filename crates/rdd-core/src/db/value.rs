//! Valor neutro de persistencia (§12.1).
//!
//! Es el único vocabulario que comparten el núcleo y los adaptadores: ningún
//! crate de motor expone sus tipos aquí (RA-014).

use crate::error::{Error, Result};

/// Un valor de columna, independiente del motor.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// SQL `NULL`, o ausencia de valor en un motor documental.
    Null,
    /// Booleano.
    Bool(bool),
    /// Entero de 64 bits con signo.
    I64(i64),
    /// Coma flotante de 64 bits.
    F64(f64),
    /// Texto UTF-8.
    Text(String),
    /// Bytes opacos.
    Bytes(Vec<u8>),
    /// Identificador UUID.
    Uuid(uuid::Uuid),
    /// JSON estructurado.
    Json(serde_json::Value),
}

impl Value {
    /// Nombre de la variante, para mensajes de error.
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Null => "Null",
            Self::Bool(_) => "Bool",
            Self::I64(_) => "I64",
            Self::F64(_) => "F64",
            Self::Text(_) => "Text",
            Self::Bytes(_) => "Bytes",
            Self::Uuid(_) => "Uuid",
            Self::Json(_) => "Json",
        }
    }

    /// Clave canónica del valor como identificador.
    ///
    /// Va etiquetada con la variante para que `Text("1")` e `I64(1)` no colisionen
    /// en los motores que indexan por clave (memoria, Redis).
    pub fn key(&self) -> String {
        match self {
            Self::Null => "null".to_owned(),
            Self::Bool(v) => format!("bool:{v}"),
            Self::I64(v) => format!("i64:{v}"),
            Self::F64(v) => format!("f64:{}", v.to_bits()),
            Self::Text(v) => format!("text:{v}"),
            Self::Bytes(v) => format!("bytes:{v:?}"),
            Self::Uuid(v) => format!("uuid:{v}"),
            Self::Json(v) => format!("json:{v}"),
        }
    }

    /// Cierto si es `Null`.
    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    /// Texto, o error de esquema si la variante no lo es.
    pub fn as_text(&self) -> Result<&str> {
        match self {
            Self::Text(v) => Ok(v),
            other => Err(other.expected("Text")),
        }
    }

    /// Booleano, o error de esquema.
    pub fn as_bool(&self) -> Result<bool> {
        match self {
            Self::Bool(v) => Ok(*v),
            other => Err(other.expected("Bool")),
        }
    }

    /// Entero, o error de esquema.
    pub fn as_i64(&self) -> Result<i64> {
        match self {
            Self::I64(v) => Ok(*v),
            other => Err(other.expected("I64")),
        }
    }

    /// Flotante, o error de esquema.
    pub fn as_f64(&self) -> Result<f64> {
        match self {
            Self::F64(v) => Ok(*v),
            other => Err(other.expected("F64")),
        }
    }

    /// UUID, o error de esquema.
    pub fn as_uuid(&self) -> Result<uuid::Uuid> {
        match self {
            Self::Uuid(v) => Ok(*v),
            other => Err(other.expected("Uuid")),
        }
    }

    /// Bytes, o error de esquema.
    pub fn as_bytes(&self) -> Result<&[u8]> {
        match self {
            Self::Bytes(v) => Ok(v),
            other => Err(other.expected("Bytes")),
        }
    }

    /// JSON, o error de esquema.
    pub fn as_json(&self) -> Result<&serde_json::Value> {
        match self {
            Self::Json(v) => Ok(v),
            other => Err(other.expected("Json")),
        }
    }

    /// RF-042: una columna cuyo tipo no coincide con el agregado es un fallo de
    /// esquema, no un 404 ni un 409; el detalle queda en el log.
    fn expected(&self, want: &str) -> Error {
        Error::internal(anyhow::anyhow!(
            "se esperaba un valor {want} y llegó {}: la columna no coincide con el agregado",
            self.type_name()
        ))
    }
}

impl From<bool> for Value {
    fn from(v: bool) -> Self {
        Self::Bool(v)
    }
}

impl From<i64> for Value {
    fn from(v: i64) -> Self {
        Self::I64(v)
    }
}

impl From<i32> for Value {
    fn from(v: i32) -> Self {
        Self::I64(i64::from(v))
    }
}

impl From<f64> for Value {
    fn from(v: f64) -> Self {
        Self::F64(v)
    }
}

impl From<String> for Value {
    fn from(v: String) -> Self {
        Self::Text(v)
    }
}

impl From<&str> for Value {
    fn from(v: &str) -> Self {
        Self::Text(v.to_owned())
    }
}

impl From<Vec<u8>> for Value {
    fn from(v: Vec<u8>) -> Self {
        Self::Bytes(v)
    }
}

impl From<uuid::Uuid> for Value {
    fn from(v: uuid::Uuid) -> Self {
        Self::Uuid(v)
    }
}

impl From<serde_json::Value> for Value {
    fn from(v: serde_json::Value) -> Self {
        Self::Json(v)
    }
}

impl<T: Into<Value>> From<Option<T>> for Value {
    fn from(v: Option<T>) -> Self {
        match v {
            Some(v) => v.into(),
            None => Self::Null,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn las_claves_no_colisionan_entre_variantes() {
        assert_ne!(Value::I64(1).key(), Value::Text("1".to_owned()).key());
        assert_ne!(Value::Bool(true).key(), Value::I64(1).key());
    }

    #[test]
    fn la_clave_es_estable() {
        let id = uuid::Uuid::nil();
        assert_eq!(Value::Uuid(id).key(), Value::Uuid(id).key());
        assert_eq!(Value::F64(0.5).key(), Value::F64(0.5).key());
    }

    #[test]
    fn option_none_es_null() {
        let v: Value = Option::<String>::None.into();
        assert!(v.is_null());
        let v: Value = Some("hola").into();
        assert_eq!(v.as_text().unwrap(), "hola");
    }

    #[test]
    fn un_tipo_inesperado_es_error_interno() {
        let err = Value::Bool(true).as_text().unwrap_err();
        assert_eq!(err.status(), crate::http::StatusCode::INTERNAL_SERVER_ERROR);
        assert!(err.to_string().contains("Text"), "{err}");
    }
}
