//! Puerto del codec de tokens (§26.3, RA-004).
//!
//! Emisión y verificación viven juntas para que el `Claims` se escriba una sola
//! vez. El adaptador HS256 está en `users/infrastructure/`; `users/mod.rs` lo
//! publica con `pub use` y así lo importa `src/middleware/jwt.rs`, que es lo que
//! RA-004 permite.

use rdd::Result;

use crate::modules::users::domain::Session;

pub trait TokenCodec: Send + Sync + 'static {
    fn issue(&self, session: &Session) -> Result<String>;

    /// `Ok(None)` ante un token que no sirve —mal formado, mal firmado, expirado
    /// o de otro algoritmo—: identificar no es rechazar, y `JwtMiddleware` sigue
    /// sin sesión (§26.3).
    fn verify(&self, token: &str) -> Result<Option<Session>>;
}
