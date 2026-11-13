//! Value objects de `files` (§26.13, RF-277).
//!
//! El nombre que llega por cabecera es una entrada del cliente: se valida aquí y
//! nunca se usa como clave. La clave la construye el caso de uso con la sesión y
//! un `Uuid` del servidor, así que no hay traversal ni acceso cruzado.

use std::fmt;

use uuid::Uuid;

/// Límite del nombre original, en bytes UTF-8.
pub const MAX_FILE_NAME_BYTES: usize = 255;

/// Nombre original validado (sin `/`, `\` ni controles).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileName(String);

/// Metadata del objeto, sin cuerpo.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileMeta {
    pub id: Uuid,
    pub size: u64,
    pub content_type: Option<String>,
}

/// Motivo por el que un nombre no sirve como nombre de archivo.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileError {
    EmptyName,
    NameTooLong { max: usize },
    InvalidChar,
}

impl FileName {
    /// Valida y recorta el nombre original.
    pub fn parse(raw: &str) -> Result<Self, FileError> {
        let name = raw.trim();
        if name.is_empty() {
            return Err(FileError::EmptyName);
        }
        if name.len() > MAX_FILE_NAME_BYTES {
            return Err(FileError::NameTooLong {
                max: MAX_FILE_NAME_BYTES,
            });
        }
        if name.contains('/') || name.contains('\\') || name.chars().any(char::is_control) {
            return Err(FileError::InvalidChar);
        }
        Ok(Self(name.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for FileName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FileMeta {
    pub fn new(id: Uuid, size: u64, content_type: Option<String>) -> Self {
        Self {
            id,
            size,
            content_type,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acepta_un_nombre_normal_y_recorta_espacios() {
        assert_eq!(
            FileName::parse("  notas.txt ").unwrap().as_str(),
            "notas.txt"
        );
        assert_eq!(
            FileName::parse("año 2026.pdf").unwrap().as_str(),
            "año 2026.pdf"
        );
    }

    #[test]
    fn rechaza_nombres_vacios_o_peligrosos() {
        for raw in [
            "",
            "   ",
            "../notas.txt",
            "sub/notas.txt",
            "sub\\notas.txt",
            "a\u{0}b",
        ] {
            assert!(FileName::parse(raw).is_err(), "aceptó {raw:?}");
        }
    }

    #[test]
    fn rechaza_un_nombre_pasado_del_limite() {
        let long = "x".repeat(MAX_FILE_NAME_BYTES + 1);
        assert_eq!(
            FileName::parse(&long).unwrap_err(),
            FileError::NameTooLong {
                max: MAX_FILE_NAME_BYTES
            }
        );
    }
}
