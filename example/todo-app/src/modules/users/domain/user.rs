//! Agregado `User` e invariantes del dominio (§7.1, §12.2, §26.5).
//!
//! `Aggregate` y `Persist` se escriben a mano hasta que `#[aggregate]` exista
//! (RF-006). El agregado no deriva `Deserialize` (RF-031): la entrada HTTP son
//! `RegisterDto` y `LoginDto`, y el hash lo produce el puerto `PasswordHasher`.

use rdd::db::{Persist, Row, Value};
use rdd::domain::Aggregate;
use uuid::Uuid;

use crate::modules::users::domain::{Role, Session};

/// Tope de RFC 5321 para la dirección completa.
pub const MAX_EMAIL_LEN: usize = 254;
/// Mínimo del demo: una contraseña corta no llega a `argon2`.
pub const MIN_PASSWORD_LEN: usize = 8;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct User {
    pub id: Uuid,
    pub email: String,
    pub password_hash: String,
    pub role: Role,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UserError {
    InvalidEmail,
    WeakPassword { min: usize },
}

impl User {
    /// Crea el agregado con un id nuevo. El hash ya viene hecho: el dominio nunca
    /// ve la contraseña en claro.
    pub fn new(email: String, password_hash: String, role: Role) -> Result<Self, UserError> {
        Ok(Self {
            id: Uuid::new_v4(),
            email: normalize_email(email)?,
            password_hash,
            role,
        })
    }

    /// La identidad que se firma en el token y se mete en las extensiones.
    pub fn session(&self) -> Session {
        Session {
            id: self.id,
            email: self.email.clone(),
            role: self.role,
        }
    }
}

impl Aggregate for User {
    type Id = Uuid;

    fn id(&self) -> &Self::Id {
        &self.id
    }
}

impl Persist for User {
    const TABLE: &'static str = "users";
    const COLUMNS: &'static [&'static str] = &["id", "email", "password_hash", "role"];

    fn to_values(&self) -> Vec<Value> {
        vec![
            Value::Uuid(self.id),
            Value::from(self.email.clone()),
            Value::from(self.password_hash.clone()),
            Value::from(self.role.as_str().to_owned()),
        ]
    }

    fn from_row(row: &Row) -> rdd::Result<Self> {
        // Por nombre y no por posición: el orden de `Row` no es contractual (§12.1).
        let role = row.value("role")?.as_text()?;
        Ok(Self {
            id: row.value("id")?.as_uuid()?,
            email: row.value("email")?.as_text()?.to_owned(),
            password_hash: row.value("password_hash")?.as_text()?.to_owned(),
            role: Role::parse(role).ok_or_else(|| {
                rdd::Error::internal(anyhow::anyhow!("rol desconocido en `users.role`: {role}"))
            })?,
        })
    }
}

/// Forma canónica del correo: sin espacios y en minúsculas, para que
/// `Ana@Correo.com` y `ana@correo.com` sean la misma cuenta.
pub fn normalize_email(email: String) -> Result<String, UserError> {
    let email = email.trim().to_lowercase();
    let mut parts = email.split('@');
    let local = parts.next().unwrap_or_default();
    let domain = parts.next().unwrap_or_default();
    let valid = parts.next().is_none()
        && !local.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && email.len() <= MAX_EMAIL_LEN
        && !email.chars().any(char::is_whitespace);
    if valid {
        Ok(email)
    } else {
        Err(UserError::InvalidEmail)
    }
}

/// La regla vive en el dominio aunque el agregado solo guarde el hash: quien la
/// aplica es el caso de uso, antes de llamar al puerto (RA-002).
pub fn validate_password(plain: &str) -> Result<(), UserError> {
    if plain.chars().count() < MIN_PASSWORD_LEN {
        return Err(UserError::WeakPassword {
            min: MIN_PASSWORD_LEN,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normaliza_el_correo() {
        assert_eq!(
            normalize_email("  Ana@Correo.COM ".to_owned()).unwrap(),
            "ana@correo.com"
        );
    }

    #[test]
    fn rechaza_correos_sin_arroba_o_sin_dominio() {
        for bad in ["", "ana", "ana@", "@correo.com", "ana@correo", "a@b@c.com"] {
            assert_eq!(
                normalize_email(bad.to_owned()).unwrap_err(),
                UserError::InvalidEmail,
                "{bad} no es un correo"
            );
        }
    }

    #[test]
    fn rechaza_una_contraseña_corta() {
        assert_eq!(
            validate_password("corta").unwrap_err(),
            UserError::WeakPassword {
                min: MIN_PASSWORD_LEN
            }
        );
        assert!(validate_password("larga123").is_ok());
    }

    #[test]
    fn sobrevive_un_round_trip_por_row() {
        let user = User::new(
            "ana@correo.com".to_owned(),
            "$argon2id$hash".to_owned(),
            Role::Admin,
        )
        .unwrap();
        let row = Row::zip(<User as Persist>::COLUMNS, &user.to_values()).unwrap();
        assert_eq!(<User as Persist>::from_row(&row).unwrap(), user);
    }

    #[test]
    fn la_sesion_no_lleva_el_hash() {
        let user = User::new(
            "ana@correo.com".to_owned(),
            "secreto".to_owned(),
            Role::User,
        )
        .unwrap();
        assert_eq!(
            user.session(),
            Session {
                id: user.id,
                email: "ana@correo.com".to_owned(),
                role: Role::User,
            }
        );
    }
}
