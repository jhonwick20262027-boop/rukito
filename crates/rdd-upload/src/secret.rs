//! Credenciales con redacción (§10.13, RF-269, RF-275).
//!
//! Un `Secret` nunca revela su valor en `Debug` ni en `Display`; para firmar hay
//! que pedirlo explícitamente con [`Secret::expose`].

use std::fmt;

/// Valor secreto que se redacta al imprimirse.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    /// Envuelve el valor leído de la configuración.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Valor en claro; solo lo llama el firmador.
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// Cierto si el valor está vacío.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(***)")
    }
}

impl fmt::Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("***")
    }
}

impl From<String> for Secret {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for Secret {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

/// Credenciales estáticas del proveedor.
///
/// `access_key` identifica la cuenta y no es secreto; `secret` y el token de
/// sesión sí, y por eso se guardan como [`Secret`].
#[derive(Clone, PartialEq, Eq)]
pub struct Credentials {
    access_key: String,
    secret: Secret,
    session_token: Option<Secret>,
}

impl Credentials {
    /// Credenciales sin token de sesión.
    pub fn new(access_key: impl Into<String>, secret: impl Into<String>) -> Self {
        Self {
            access_key: access_key.into(),
            secret: Secret::new(secret),
            session_token: None,
        }
    }

    /// Añade el token de sesión temporal (S3).
    pub fn with_session_token(mut self, token: impl Into<String>) -> Self {
        self.session_token = Some(Secret::new(token));
        self
    }

    /// Identificador de la credencial.
    pub fn access_key(&self) -> &str {
        &self.access_key
    }

    /// Secreto de firma.
    pub fn secret(&self) -> &Secret {
        &self.secret
    }

    /// Token de sesión, si lo hay.
    pub fn session_token(&self) -> Option<&Secret> {
        self.session_token.as_ref()
    }
}

impl fmt::Debug for Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Credentials")
            .field("access_key", &self.access_key)
            .field("secret", &"***")
            .field("session_token", &self.session_token.as_ref().map(|_| "***"))
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_y_display_no_revelan_el_secreto() {
        let secret = Secret::new("wJalrXUtnFEMI/K7MDENG");
        assert!(!format!("{secret:?}").contains("wJalr"));
        assert!(!format!("{secret}").contains("wJalr"));
        assert_eq!(secret.expose(), "wJalrXUtnFEMI/K7MDENG");
    }

    #[test]
    fn las_credenciales_redactan_el_secreto_y_el_token() {
        let credentials =
            Credentials::new("AKIAIOSFODNN7EXAMPLE", "secreto").with_session_token("tok-123");
        let debug = format!("{credentials:?}");
        assert!(debug.contains("AKIAIOSFODNN7EXAMPLE"));
        assert!(!debug.contains("secreto"));
        assert!(!debug.contains("tok-123"));
    }
}
