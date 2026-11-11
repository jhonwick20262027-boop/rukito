//! Casos de uso de `users` (§5.1, RA-002, §26.3).
//!
//! Coordina dominio y puertos. No nombra ni `argon2` ni `jsonwebtoken`: los
//! recibe como `dyn` para que la raíz de composición decida el adaptador y para
//! que un test pueda sustituirlos sin tocar esta capa.

use std::sync::Arc;

use rdd::prelude::*;
use uuid::Uuid;

use crate::modules::users::domain::{
    PasswordHasher, Role, TokenCodec, User, UserError, UserRepository, normalize_email,
    validate_password,
};

pub struct UsersUseCases<R: UserRepository> {
    repo: R,
    hasher: Arc<dyn PasswordHasher>,
    tokens: Arc<dyn TokenCodec>,
    /// Correo que nace `admin` (§26.3). `None` si `ADMIN_EMAIL` no está puesta.
    bootstrap_admin: Option<String>,
}

impl<R: UserRepository> UsersUseCases<R> {
    pub fn new(
        repo: R,
        hasher: Arc<dyn PasswordHasher>,
        tokens: Arc<dyn TokenCodec>,
        bootstrap_admin: Option<String>,
    ) -> Self {
        Self {
            repo,
            hasher,
            tokens,
            bootstrap_admin,
        }
    }

    /// Alta de una cuenta. El rol NO viene de la petición: dejarlo elegir al
    /// cliente sería una escalada de privilegios (§26.3).
    pub async fn register(&self, email: String, password: String) -> Result<User> {
        validate_password(&password).map_err(domain_error)?;
        let email = normalize_email(email).map_err(domain_error)?;
        if self.repo.find_by_email(&email).await?.is_some() {
            return Err(Error::conflict("ya hay una cuenta con ese correo"));
        }
        let role = match &self.bootstrap_admin {
            Some(admin) if admin.trim().eq_ignore_ascii_case(&email) => Role::Admin,
            _ => Role::User,
        };
        let hash = self.hasher.hash(&password)?;
        let user = User::new(email, hash, role).map_err(domain_error)?;
        self.repo.save(&user).await?;
        Ok(user)
    }

    /// Devuelve la cuenta y su token. El controller no vuelve a mirar la
    /// contraseña ni a firmar nada.
    pub async fn login(&self, email: String, password: String) -> Result<(User, String)> {
        let email = normalize_email(email).map_err(domain_error)?;
        // 401 tanto si la cuenta no existe como si la contraseña no coincide:
        // distinguirlos convertiría el login en un oráculo de correos dados de
        // alta.
        let user = self
            .repo
            .find_by_email(&email)
            .await?
            .ok_or(Error::Unauthorized)?;
        if !self.hasher.verify(&password, &user.password_hash)? {
            return Err(Error::Unauthorized);
        }
        let token = self.tokens.issue(&user.session())?;
        Ok((user, token))
    }

    /// La cuenta de la sesión actual. El id viene de `Ext<Session>`, nunca de la
    /// ruta: pedir `/users/<otro-id>` no puede leer datos ajenos.
    pub async fn me(&self, id: Uuid) -> Result<User> {
        self.repo.get(id).await
    }
}

fn domain_error(error: UserError) -> Error {
    match error {
        UserError::InvalidEmail => Error::invalid("el correo no es válido"),
        UserError::WeakPassword { min } => Error::invalid(format!(
            "la contraseña debe tener al menos {min} caracteres"
        )),
    }
}
