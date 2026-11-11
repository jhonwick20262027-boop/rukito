//! Tipos de entrada y salida HTTP para `tasks`.
//!
//! El agregado no se deserializa desde la petición (RF-031): estos DTOs son la
//! única forma de entrada, y `TaskResponse` la de salida.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::modules::tasks::domain::Task;

#[derive(Debug, Deserialize)]
pub struct QueryTaskDto {
    pub q: Option<String>,
    pub done: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct CreateTaskDto {
    pub title: String,
    pub done: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateTaskDto {
    pub title: Option<String>,
    pub done: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct TaskResponse {
    pub id: Uuid,
    pub title: String,
    pub done: bool,
    pub owner_id: Uuid,
}

impl From<Task> for TaskResponse {
    fn from(item: Task) -> Self {
        Self {
            id: item.id,
            title: item.title,
            done: item.done,
            owner_id: item.owner_id,
        }
    }
}
