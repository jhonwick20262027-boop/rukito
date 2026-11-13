//! Política de claves del puerto (§10.13, RF-270).
//!
//! Un adaptador solo recibe claves ya validadas; la ruta firmada se construye
//! codificando cada segmento, nunca interpolando texto del cliente.

use std::fmt;

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};

/// Caracteres que SigV4 y GCS dejan sin codificar (RFC 3986, unreserved).
const UNRESERVED: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

/// Clave de objeto validada.
///
/// Acepta de 1 a 1024 bytes UTF-8 sin `/` inicial, sin segmentos vacíos, `.` o
/// `..`, sin `\` y sin caracteres de control. `/` separa segmentos y se conserva.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObjectKey(String);

/// Motivo por el que una clave no cumple la política (RF-270).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ObjectKeyError {
    /// La clave está vacía.
    #[error("la clave no puede estar vacía")]
    Empty,
    /// Supera el máximo del puerto.
    #[error("la clave supera los 1024 bytes")]
    TooLong,
    /// Empieza por `/`.
    #[error("la clave no puede empezar por `/`")]
    LeadingSlash,
    /// Tiene un segmento vacío (`//` o `/` final).
    #[error("la clave tiene un segmento vacío")]
    EmptySegment,
    /// Tiene un segmento `.` o `..`.
    #[error("la clave no puede contener los segmentos `.` ni `..`")]
    Traversal,
    /// Contiene `\`, que algunos proveedores interpretan como separador.
    #[error("la clave no puede contener `\\`")]
    Backslash,
    /// Contiene un carácter de control.
    #[error("la clave no puede contener caracteres de control")]
    Control,
}

impl ObjectKey {
    /// Longitud máxima en bytes UTF-8.
    pub const MAX_BYTES: usize = 1024;

    /// Valida la clave y la devuelve envuelta.
    pub fn parse(raw: impl Into<String>) -> Result<Self, ObjectKeyError> {
        let raw = raw.into();
        if raw.is_empty() {
            return Err(ObjectKeyError::Empty);
        }
        if raw.len() > Self::MAX_BYTES {
            return Err(ObjectKeyError::TooLong);
        }
        if raw.starts_with('/') {
            return Err(ObjectKeyError::LeadingSlash);
        }
        if raw.contains('\\') {
            return Err(ObjectKeyError::Backslash);
        }
        if raw.chars().any(|ch| ch.is_control()) {
            return Err(ObjectKeyError::Control);
        }
        for segment in raw.split('/') {
            if segment.is_empty() {
                return Err(ObjectKeyError::EmptySegment);
            }
            if segment == "." || segment == ".." {
                return Err(ObjectKeyError::Traversal);
            }
        }
        Ok(Self(raw))
    }

    /// Clave sin codificar.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Ruta codificada por segmento, conservando `/` como separador.
    pub fn encoded_path(&self) -> String {
        self.0
            .split('/')
            .map(|segment| utf8_percent_encode(segment, UNRESERVED).to_string())
            .collect::<Vec<_>>()
            .join("/")
    }
}

impl fmt::Display for ObjectKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for ObjectKey {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acepta_claves_anidadas_seguras() {
        let key = ObjectKey::parse("files/123/notas.txt").unwrap();
        assert_eq!(key.as_str(), "files/123/notas.txt");
        assert_eq!(key.encoded_path(), "files/123/notas.txt");
    }

    #[test]
    fn rechaza_las_claves_peligrosas() {
        for raw in [
            "",
            "/absoluta",
            "a//b",
            "a/",
            "a/./b",
            "a/../b",
            "a\\b",
            "a\u{0}b",
        ] {
            assert!(ObjectKey::parse(raw).is_err(), "aceptó {raw:?}");
        }
        let largo = "a".repeat(ObjectKey::MAX_BYTES + 1);
        assert_eq!(
            ObjectKey::parse(largo).unwrap_err(),
            ObjectKeyError::TooLong
        );
    }

    #[test]
    fn codifica_por_segmento_y_conserva_la_barra() {
        let key = ObjectKey::parse("mis archivos/año 1.txt").unwrap();
        assert_eq!(key.encoded_path(), "mis%20archivos/a%C3%B1o%201.txt");
    }
}
