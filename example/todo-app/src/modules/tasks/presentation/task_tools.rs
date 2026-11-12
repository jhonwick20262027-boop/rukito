//! Tools MCP del módulo `tasks` (§26.7, RA-019).
//!
//! Son adaptadores de entrada como el controller: traducen argumentos, leen la
//! identidad del `ToolContext` (ADR-25) y llaman al alias `TasksService`. No
//! tocan `Repo` ni `infrastructure` (RA-003): el dueño viaja en las extensiones
//! de la petición, así que el mismo registro sirve a stdio y a `/mcp`.

use rdd::mcp::{CallToolResult, Tool, ToolContext};
use rdd::prelude::*;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::modules::tasks::TasksService;
use crate::modules::tasks::presentation::dto::TaskResponse;
use crate::modules::users::Session;

/// `create_task`: crea una tarea del usuario autenticado.
pub struct CreateTask;

impl Tool for CreateTask {
    fn name(&self) -> &str {
        "create_task"
    }

    fn description(&self) -> &str {
        "Crea una tarea del usuario autenticado."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": { "title": { "type": "string", "maxLength": 200 } },
            "required": ["title"],
            "additionalProperties": false
        })
    }

    fn call(
        &self,
        args: Value,
        ctx: ToolContext,
    ) -> rdd::http::BoxFuture<'static, rdd::Result<CallToolResult>> {
        Box::pin(async move {
            let session = ctx.ext::<Session>().ok_or(Error::Unauthorized)?;
            let svc = ctx.resolve::<TasksService>()?;
            let title = args
                .get("title")
                .and_then(Value::as_str)
                .ok_or_else(|| Error::invalid("falta `title`"))?;
            let task = svc.create(session.id, title.to_owned(), None).await?;
            let body = serde_json::to_value(TaskResponse::from(task))
                .map_err(|err| Error::internal(err.into()))?;
            Ok(CallToolResult::json(body))
        })
    }
}

/// `list_tasks`: devuelve las tareas del usuario, con filtro opcional por estado.
pub struct ListTasks;

impl Tool for ListTasks {
    fn name(&self) -> &str {
        "list_tasks"
    }

    fn description(&self) -> &str {
        "Lista las tareas del usuario autenticado."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": { "done": { "type": "boolean" } },
            "additionalProperties": false
        })
    }

    fn call(
        &self,
        args: Value,
        ctx: ToolContext,
    ) -> rdd::http::BoxFuture<'static, rdd::Result<CallToolResult>> {
        Box::pin(async move {
            let session = ctx.ext::<Session>().ok_or(Error::Unauthorized)?;
            let svc = ctx.resolve::<TasksService>()?;
            let done = match args.get("done") {
                None | Some(Value::Null) => None,
                Some(Value::Bool(done)) => Some(*done),
                Some(_) => return Err(Error::invalid("`done` debe ser booleano")),
            };
            let tasks = svc.list_of(session.id, None, done).await?;
            let body = serde_json::to_value(
                tasks
                    .into_iter()
                    .map(TaskResponse::from)
                    .collect::<Vec<_>>(),
            )
            .map_err(|err| Error::internal(err.into()))?;
            Ok(CallToolResult::json(body))
        })
    }
}

/// `complete_task`: marca como hecha una tarea propia.
pub struct CompleteTask;

impl Tool for CompleteTask {
    fn name(&self) -> &str {
        "complete_task"
    }

    fn description(&self) -> &str {
        "Marca como hecha una tarea del usuario autenticado."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": { "id": { "type": "string", "format": "uuid" } },
            "required": ["id"],
            "additionalProperties": false
        })
    }

    fn call(
        &self,
        args: Value,
        ctx: ToolContext,
    ) -> rdd::http::BoxFuture<'static, rdd::Result<CallToolResult>> {
        Box::pin(async move {
            let session = ctx.ext::<Session>().ok_or(Error::Unauthorized)?;
            let svc = ctx.resolve::<TasksService>()?;
            let id = args
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| Error::invalid("falta `id`"))?;
            let id = Uuid::parse_str(id).map_err(|_| Error::invalid("`id` debe ser un UUID"))?;
            let task = svc.update_of(session.id, id, None, Some(true)).await?;
            let body = serde_json::to_value(TaskResponse::from(task))
                .map_err(|err| Error::internal(err.into()))?;
            Ok(CallToolResult::json(body))
        })
    }
}
