//! Adaptador de persistencia de `Task` sobre `Repo<T>` (§12.1, RA-007).
//!
//! No conoce ningún driver: el motor lo decide la URL en el arranque (§12.6), así
//! que cambiar de PostgreSQL a otro no toca este archivo ni el caso de uso
//! (RF-185). Tampoco tiene almacén propio ni `static`: el `Repo` se inyecta.

use rdd::prelude::*;
use uuid::Uuid;

use crate::modules::tasks::domain::Task;
use crate::modules::tasks::domain::TaskRepository;

/// Implementa el puerto del dominio con el handle agnóstico del núcleo.
pub struct TasksRepository {
    repo: Repo<Task>,
}

impl TasksRepository {
    /// Recibe el `Repo` ya resuelto del contenedor (§12.1).
    pub fn new(repo: Repo<Task>) -> Self {
        Self { repo }
    }
}

impl TaskRepository for TasksRepository {
    async fn list(&self) -> Result<Vec<Task>> {
        self.repo.all().await
    }

    async fn get(&self, id: Uuid) -> Result<Task> {
        self.repo.get(id).await
    }

    async fn save(&self, item: &Task) -> Result<()> {
        self.repo.save(item).await
    }

    async fn remove(&self, id: Uuid) -> Result<()> {
        self.repo.delete(id).await
    }
}
