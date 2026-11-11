//! Adaptador `argon2` del puerto `PasswordHasher` (§26.3, RA-007).
//!
//! Argon2id con los parámetros por defecto de la crate: memoria dura, que es lo
//! que hace caro un ataque por fuerza bruta sobre la base filtrada. La sal es
//! aleatoria por contraseña, nunca derivada del correo.

use argon2::password_hash::{PasswordHash, SaltString, rand_core::OsRng};
use argon2::{Argon2, PasswordHasher as _, PasswordVerifier as _};
use rdd::prelude::*;

use crate::modules::users::domain::PasswordHasher;

/// Sin estado: los parámetros son los de `Argon2::default()`.
pub struct Argon2Hasher;

impl PasswordHasher for Argon2Hasher {
    fn hash(&self, plain: &str) -> Result<String> {
        let salt = SaltString::generate(&mut OsRng);
        Argon2::default()
            .hash_password(plain.as_bytes(), &salt)
            .map(|hash| hash.to_string())
            .map_err(|err| Error::internal(anyhow::anyhow!("argon2 no pudo hashear: {err}")))
    }

    fn verify(&self, plain: &str, hash: &str) -> Result<bool> {
        let parsed = PasswordHash::new(hash)
            .map_err(|err| Error::internal(anyhow::anyhow!("hash guardado inválido: {err}")))?;
        // `verify_password` devuelve `Err` tanto si no coincide como si el
        // algoritmo no es soportado; para el login los dos casos son «no entra».
        Ok(Argon2::default()
            .verify_password(plain.as_bytes(), &parsed)
            .is_ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_misma_contraseña_da_dos_hashes_distintos() {
        let hasher = Argon2Hasher;
        let first = hasher.hash("larga123").unwrap();
        let second = hasher.hash("larga123").unwrap();
        assert_ne!(first, second, "la sal debe ser aleatoria");
        assert!(first.starts_with("$argon2id$"), "{first}");
        assert!(hasher.verify("larga123", &first).unwrap());
        assert!(hasher.verify("larga123", &second).unwrap());
    }

    #[test]
    fn una_contraseña_equivocada_no_verifica() {
        let hasher = Argon2Hasher;
        let hash = hasher.hash("larga123").unwrap();
        assert!(!hasher.verify("otra-cosa", &hash).unwrap());
    }

    #[test]
    fn un_hash_corrupto_es_error_interno() {
        let err = Argon2Hasher
            .verify("larga123", "no-es-un-hash")
            .unwrap_err();
        assert_eq!(err.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }
}
