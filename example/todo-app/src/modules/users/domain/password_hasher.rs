//! Puerto de hashing de contraseñas (§26.3, RA-002).
//!
//! Síncrono a propósito: `argon2` es CPU-bound y no hace I/O, así que no hay
//! futuro que devolver. El caso de uso llama al puerto y nunca nombra el crate.

use rdd::Result;

pub trait PasswordHasher: Send + Sync + 'static {
    /// Hash con sal aleatoria: dos registros de la misma contraseña no producen
    /// el mismo texto.
    fn hash(&self, plain: &str) -> Result<String>;

    /// `false` si no coincide; `Err` solo si el hash guardado está corrupto.
    fn verify(&self, plain: &str, hash: &str) -> Result<bool>;
}
