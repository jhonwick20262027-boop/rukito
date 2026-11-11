//! Puerto de persistencia del agregado `Task` (§5.1, RA-001).
//!
//! Async con `impl Future + Send`, la misma forma de `FromRequest` (§6.3): un
//! `async fn` en el trait no garantiza que el futuro sea `Send`, y los handlers de
//! la plataforma sí lo exigen. El adaptador lo implementa con `async fn`.

use std::future::Future;

use rdd::Result;
use uuid::Uuid;

use crate::modules::tasks::domain::Task;

pub trait TaskRepository: Send + Sync + 'static {
    fn list(&self) -> impl Future<Output = Result<Vec<Task>>> + Send;
    fn get(&self, id: Uuid) -> impl Future<Output = Result<Task>> + Send;
    fn save(&self, item: &Task) -> impl Future<Output = Result<()>> + Send;
    fn remove(&self, id: Uuid) -> impl Future<Output = Result<()>> + Send;
}
