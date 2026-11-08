//! Controller de `tasks` (§8.2).
//!
//! Los handlers reciben el repositorio de tareas desde el contenedor de la app;
//! las reglas de negocio se delegan a `TasksService`.

use rdd::controller;
use rdd::prelude::*;

use crate::modules::tasks::dto::create_task_dto::CreateTaskDto;
use crate::modules::tasks::dto::query_task_dto::QueryTaskDto;
use crate::modules::tasks::dto::update_task_dto::UpdateTaskDto;
use crate::modules::tasks::entities::task_entity::Task;
use crate::modules::tasks::repositories::tasks_repository::TasksRepository;
use crate::modules::tasks::services::tasks_service::TasksService;

pub struct TasksController;

#[controller("/tasks")]
impl TasksController {
    /// `GET /tasks` — lista, con filtro opcional por texto y estado.
    #[get("/")]
    async fn list(repo: TasksRepository, query: Query<QueryTaskDto>) -> Result<Vec<Task>> {
        TasksService::list(repo.0, query.into_inner()).await
    }

    /// `GET /tasks/:id` — 404 en Problem Details si no existe (§10.3).
    #[get("/:id")]
    async fn get(id: Path<uuid::Uuid>, repo: TasksRepository) -> Result<Task> {
        TasksService::get(repo.0, *id).await
    }

    /// `POST /tasks` — 201 con el recurso creado (RF-053).
    #[post("/", status = 201)]
    async fn create(body: Json<CreateTaskDto>, repo: TasksRepository) -> Result<Task> {
        TasksService::create(repo.0, body.into_inner()).await
    }

    /// `PUT /tasks/:id` — actualización parcial.
    #[put("/:id")]
    async fn update(
        id: Path<uuid::Uuid>,
        body: Json<UpdateTaskDto>,
        repo: TasksRepository,
    ) -> Result<Task> {
        TasksService::update(repo.0, *id, body.into_inner()).await
    }

    /// `DELETE /tasks/:id` — 204 sin cuerpo (RF-053).
    #[delete("/:id")]
    async fn remove(id: Path<uuid::Uuid>, repo: TasksRepository) -> Result<()> {
        TasksService::remove(repo.0, *id).await
    }
}
