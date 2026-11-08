//! Reglas de negocio de `tasks`.
//!
//! El controller no decide nada: aquí se valida y se traduce a `Error`, que el
//! núcleo convierte en Problem Details (§10.3).

use rdd::prelude::*;
use uuid::Uuid;

use crate::modules::tasks::constants::tasks_constants::MAX_TITLE_LEN;
use crate::modules::tasks::dto::create_task_dto::CreateTaskDto;
use crate::modules::tasks::dto::query_task_dto::QueryTaskDto;
use crate::modules::tasks::dto::update_task_dto::UpdateTaskDto;
use crate::modules::tasks::entities::task_entity::Task;
pub struct TasksService;

impl TasksService {
    pub async fn list(repo: Repo<Task>, query: QueryTaskDto) -> Result<Vec<Task>> {
        let mut tasks = repo.all().await?;
        tasks.retain(|task| {
            query
                .q
                .as_ref()
                .is_none_or(|text| task.title.contains(text))
                && query.done.is_none_or(|done| task.done == done)
        });
        Ok(tasks)
    }

    pub async fn get(repo: Repo<Task>, id: Uuid) -> Result<Task> {
        repo.get(id).await
    }

    pub async fn create(repo: Repo<Task>, dto: CreateTaskDto) -> Result<Task> {
        let task = Task {
            id: Uuid::new_v4(),
            title: validate_title(&dto.title)?,
            done: dto.done.unwrap_or(false),
        };
        repo.save(&task).await?;
        Ok(task)
    }

    pub async fn update(repo: Repo<Task>, id: Uuid, dto: UpdateTaskDto) -> Result<Task> {
        let mut task = repo.get(id).await?;
        if let Some(title) = dto.title {
            task.title = validate_title(&title)?;
        }
        if let Some(done) = dto.done {
            task.done = done;
        }
        repo.save(&task).await?;
        Ok(task)
    }

    pub async fn remove(repo: Repo<Task>, id: Uuid) -> Result<()> {
        repo.delete(id).await
    }
}

/// El título no va vacío ni pasa de `MAX_TITLE_LEN`.
///
/// Vacío es 400 (petición mal formada); demasiado largo es 422 (`Invalid`), que
/// es la distinción que hace §10.3.
fn validate_title(title: &str) -> Result<String> {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        return Err(Error::bad_request("el título no puede estar vacío"));
    }
    if trimmed.chars().count() > MAX_TITLE_LEN {
        return Err(Error::invalid(format!(
            "el título no puede pasar de {MAX_TITLE_LEN} caracteres"
        )));
    }
    Ok(trimmed.to_owned())
}
