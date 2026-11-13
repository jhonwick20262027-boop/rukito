//! Error de configuración de los adaptadores (§10.13, RF-275).
//!
//! Un secreto ausente o un endpoint inválido son fallos del operador: el mensaje
//! nombra la variable o el valor, nunca la credencial.

/// Fallo al construir una configuración desde el entorno.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum UploadConfigError {
    /// Falta una variable obligatoria.
    #[error("falta la variable de entorno `{0}`")]
    Missing(&'static str),
    /// Una variable existe pero no tiene un valor utilizable.
    #[error("la variable `{name}` no es válida: {reason}")]
    Invalid {
        /// Nombre de la variable.
        name: &'static str,
        /// Motivo legible.
        reason: String,
    },
    /// El endpoint no es `https` ni un `http` explícito de emulador.
    #[error("el endpoint `{endpoint}` no es válido: {reason}")]
    Endpoint {
        /// Valor configurado.
        endpoint: String,
        /// Motivo legible.
        reason: &'static str,
    },
    /// El almacén no puede usarse con esos valores (bucket, cuenta, región).
    #[error("{0}")]
    Config(String),
}

#[cfg(any(feature = "aws", feature = "azure", feature = "gcp"))]
impl UploadConfigError {
    pub(crate) fn invalid(name: &'static str, reason: impl Into<String>) -> Self {
        Self::Invalid {
            name,
            reason: reason.into(),
        }
    }

    pub(crate) fn endpoint(endpoint: impl Into<String>, reason: &'static str) -> Self {
        Self::Endpoint {
            endpoint: endpoint.into(),
            reason,
        }
    }
}
