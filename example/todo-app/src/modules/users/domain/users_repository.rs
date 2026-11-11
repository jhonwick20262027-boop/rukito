//! Puerto de persistencia del agregado `User` (§5.1, RA-001).
//!
//! Async con `impl Future + Send`, la misma forma de `FromRequest` (§6.3): un
//! `async fn` en el trait no garantiza que el futuro sea `Send`, y los handlers
//! de la plataforma sí lo exigen. El adaptador lo implementa con `async fn`.

use std::future::Future;

use rdd::Result;
use uuid::Uuid;

use crate::modules::users::domain::User;

pub trait UserRepository: Send + Sync + 'static {
    /// `None` si no existe: el login decide qué responder, no el adaptador.
    fn find_by_email(&self, email: &str) -> impl Future<Output = Result<Option<User>>> + Send;
    fn get(&self, id: Uuid) -> impl Future<Output = Result<User>> + Send;
    fn save(&self, user: &User) -> impl Future<Output = Result<()>> + Send;
}
