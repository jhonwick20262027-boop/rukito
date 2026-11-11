//! Adaptador de persistencia de `User` sobre `Repo<T>` (§12.1, RA-007).
//!
//! No conoce ningún driver: el motor lo decide la URL en el arranque (§12.6).
//! Tampoco tiene almacén propio ni `static`: el `Repo` se inyecta.

use rdd::prelude::*;
use uuid::Uuid;

use crate::modules::users::domain::{User, UserRepository};

pub struct UsersRepository {
    repo: Repo<User>,
}

impl UsersRepository {
    /// Recibe el `Repo` ya resuelto del contenedor (§12.1).
    pub fn new(repo: Repo<User>) -> Self {
        Self { repo }
    }
}

impl UserRepository for UsersRepository {
    async fn find_by_email(&self, email: &str) -> Result<Option<User>> {
        // `Repo<T>` solo consulta por id (§12.1), así que el demo recorre la
        // tabla. Una app real usaría el handle nativo con su índice y la
        // supresión `// rdd:allow(RA-007) motivo` que exige RF-115.
        // La carrera entre dos registros del mismo correo no la cubre este
        // `find`: la cubre el `UNIQUE` de `users.email`, cuyo choque el adaptador
        // traduce a 409 (RF-042).
        Ok(self
            .repo
            .all()
            .await?
            .into_iter()
            .find(|user| user.email == email))
    }

    async fn get(&self, id: Uuid) -> Result<User> {
        self.repo.get(id).await
    }

    async fn save(&self, user: &User) -> Result<()> {
        self.repo.save(user).await
    }
}
