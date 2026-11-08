//! Mapper de `Task`.

use crate::modules::tasks::entities::task_entity::Task;
use uuid::Uuid;

pub struct TaskMapper;

impl TaskMapper {
    /// DTO de creación → agregado. El título ya viene validado del service.
    pub fn from_create(id: Uuid, title: String, done: bool) -> Task {
        Task { id, title, done }
    }

    /// Aplica una actualización parcial sobre el agregado actual.
    pub fn apply_update(current: Task, title: String, done: Option<bool>) -> Task {
        Task {
            title,
            done: done.unwrap_or(current.done),
            ..current
        }
    }
}
