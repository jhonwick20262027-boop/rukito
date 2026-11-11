//! Controller de `tasks` (§8.2, RA-003, §26.3).
//!
//! Traduce HTTP a llamadas de aplicación y convierte el resultado del dominio
//! al DTO de respuesta. Usa el alias público del módulo, nunca el adaptador de
//! persistencia (§14.2).
//!
//! `RequireAuth` va en el `impl` y aplica a las cinco rutas; `RequireRole` solo
//! en `DELETE`. El orden efectivo lo fija RF-080: global → módulo → controller →
//! ruta, así que la identidad ya está en las extensiones cuando estos dos corren.

use rdd::controller;
use rdd::prelude::*;
use uuid::Uuid;

use crate::middleware::{RequireAuth, RequireRole};
use crate::modules::tasks::TasksService;
use crate::modules::tasks::presentation::dto::{
    CreateTaskDto, QueryTaskDto, TaskResponse, UpdateTaskDto,
};
use crate::modules::users::{Role, Session};

pub struct TasksController;

#[controller("/tasks")]
#[middleware(RequireAuth)]
impl TasksController {
    /// `GET /tasks` — solo las del dueño, con filtro opcional por texto y estado.
    #[get("/")]
    async fn list(
        svc: TasksService,
        session: Ext<Session>,
        query: Query<QueryTaskDto>,
    ) -> Result<Vec<TaskResponse>> {
        let query = query.into_inner();
        Ok(svc
            .list_of(session.id, query.q, query.done)
            .await?
            .into_iter()
            .map(TaskResponse::from)
            .collect())
    }

    /// `GET /tasks/:id` — 404 si no existe o es de otro (§26.6, comprobación 8).
    #[get("/:id")]
    async fn get(svc: TasksService, session: Ext<Session>, id: Path<Uuid>) -> Result<TaskResponse> {
        svc.get_of(session.id, *id).await.map(TaskResponse::from)
    }

    /// `POST /tasks` — 201; el dueño sale de la sesión, nunca del cuerpo (RF-053).
    #[post("/", status = 201)]
    async fn create(
        svc: TasksService,
        session: Ext<Session>,
        body: Json<CreateTaskDto>,
    ) -> Result<TaskResponse> {
        let body = body.into_inner();
        svc.create(session.id, body.title, body.done)
            .await
            .map(TaskResponse::from)
    }

    /// `PUT /tasks/:id` — actualización parcial, siempre sobre una tarea propia.
    #[put("/:id")]
    async fn update(
        svc: TasksService,
        session: Ext<Session>,
        id: Path<Uuid>,
        body: Json<UpdateTaskDto>,
    ) -> Result<TaskResponse> {
        let body = body.into_inner();
        svc.update_of(session.id, *id, body.title, body.done)
            .await
            .map(TaskResponse::from)
    }

    /// `DELETE /tasks/:id` — 204 sin cuerpo. Exige `admin` además de ser dueño.
    #[delete("/:id")]
    #[middleware(RequireRole::new(Role::Admin))]
    async fn remove(svc: TasksService, session: Ext<Session>, id: Path<Uuid>) -> Result<()> {
        svc.remove_of(session.id, *id).await
    }
}
