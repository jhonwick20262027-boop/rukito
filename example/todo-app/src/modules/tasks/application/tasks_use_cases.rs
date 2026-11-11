//! Casos de uso de `tasks` (§5.1, RA-002, §26.5).
//!
//! Coordina operaciones de dominio y expone una API independiente de HTTP. El
//! filtro por dueño vive aquí: el controller no decide nada (§5.1) y el puerto
//! de persistencia no sabe quién pregunta.

use rdd::prelude::*;
use uuid::Uuid;

use crate::modules::tasks::domain::TaskRepository;
use crate::modules::tasks::domain::{Task, TaskError};

/// Orquesta los casos de uso; las invariantes pertenecen al agregado.
///
/// Sin tipo por defecto: nombrar el adaptador aquí violaría RA-002. Quien lo fija
/// es la raíz de composición del módulo con el alias `TasksService` (§14.2).
pub struct TasksUseCases<R: TaskRepository> {
    repo: R,
}

impl<R: TaskRepository> TasksUseCases<R> {
    /// Constructor sobre cualquier implementación del puerto del dominio.
    pub fn with_repository(repo: R) -> Self {
        Self { repo }
    }

    pub async fn list_of(
        &self,
        owner: Uuid,
        q: Option<String>,
        done: Option<bool>,
    ) -> Result<Vec<Task>> {
        Ok(self
            .repo
            .list()
            .await?
            .into_iter()
            .filter(|row| row.belongs_to(owner))
            .filter(|row| match &q {
                Some(text) => row.title.contains(text.as_str()),
                None => true,
            })
            .filter(|row| done.is_none_or(|done| row.done == done))
            .collect())
    }

    pub async fn get_of(&self, owner: Uuid, id: Uuid) -> Result<Task> {
        self.owned(owner, id).await
    }

    pub async fn create(&self, owner: Uuid, title: String, done: Option<bool>) -> Result<Task> {
        let item = Task::new(owner, title, done.unwrap_or(false)).map_err(domain_error)?;
        self.repo.save(&item).await?;
        Ok(item)
    }

    pub async fn update_of(
        &self,
        owner: Uuid,
        id: Uuid,
        title: Option<String>,
        done: Option<bool>,
    ) -> Result<Task> {
        let mut current = self.owned(owner, id).await?;
        current.update(title, done).map_err(domain_error)?;
        self.repo.save(&current).await?;
        Ok(current)
    }

    /// El rol `admin` autoriza la operación; el dueño autoriza el recurso. Un
    /// admin tampoco borra tareas ajenas (§26.5).
    pub async fn remove_of(&self, owner: Uuid, id: Uuid) -> Result<()> {
        let item = self.owned(owner, id).await?;
        self.repo.remove(item.id).await
    }

    /// 404 y no 403 cuando la tarea es de otro: un 403 confirmaría que existe
    /// (§26.6, comprobación 8).
    async fn owned(&self, owner: Uuid, id: Uuid) -> Result<Task> {
        let item = self.repo.get(id).await?;
        if !item.belongs_to(owner) {
            return Err(Error::not_found(format!("no existe la tarea {id}")));
        }
        Ok(item)
    }
}

fn domain_error(error: TaskError) -> Error {
    match error {
        TaskError::EmptyTitle => Error::bad_request("el título no puede estar vacío"),
        TaskError::TitleTooLong { max } => {
            Error::invalid(format!("el título no puede pasar de {max} caracteres"))
        }
    }
}
