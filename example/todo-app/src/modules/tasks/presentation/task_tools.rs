//! Tools MCP del módulo `tasks` (§26.7, RA-019).
//!
//! Son adaptadores de entrada como el controller: traducen argumentos, llaman al
//! alias `TasksService` y devuelven contenido. No tocan `Repo` ni `infrastructure`
//! (RA-003) y el dueño viene del proceso, nunca de los argumentos.

use std::sync::Arc;

use rdd::mcp::{CallToolResult, Tool, ToolContext};
use rdd::prelude::*;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::modules::tasks::TasksService;
use crate::modules::tasks::presentation::dto::TaskResponse;

/// `create_task`: crea una tarea del dueño del proceso.
pub struct CreateTask {
    svc: Arc<TasksService>,
    owner: Uuid,
}

impl CreateTask {
    pub fn new(svc: Arc<TasksService>, owner: Uuid) -> Self {
        Self { svc, owner }
    }
}

impl Tool for CreateTask {
    fn name(&self) -> &str {
        "create_task"
    }

    fn description(&self) -> &str {
        "Crea una tarea del dueño del proceso."
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
        _ctx: ToolContext,
    ) -> rdd::http::BoxFuture<'static, rdd::Result<CallToolResult>> {
        let svc = Arc::clone(&self.svc);
        let owner = self.owner;
        Box::pin(async move {
            let title = args
                .get("title")
                .and_then(Value::as_str)
                .ok_or_else(|| Error::invalid("falta `title`"))?;
            let task = svc.create(owner, title.to_owned(), None).await?;
            let body = serde_json::to_value(TaskResponse::from(task))
                .map_err(|err| Error::internal(err.into()))?;
            Ok(CallToolResult::json(body))
        })
    }
}

/// `list_tasks`: devuelve las tareas del dueño, con filtro opcional por estado.
pub struct ListTasks {
    svc: Arc<TasksService>,
    owner: Uuid,
}

impl ListTasks {
    pub fn new(svc: Arc<TasksService>, owner: Uuid) -> Self {
        Self { svc, owner }
    }
}

impl Tool for ListTasks {
    fn name(&self) -> &str {
        "list_tasks"
    }

    fn description(&self) -> &str {
        "Lista las tareas del dueño del proceso."
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
        _ctx: ToolContext,
    ) -> rdd::http::BoxFuture<'static, rdd::Result<CallToolResult>> {
        let svc = Arc::clone(&self.svc);
        let owner = self.owner;
        Box::pin(async move {
            let done = match args.get("done") {
                None | Some(Value::Null) => None,
                Some(Value::Bool(done)) => Some(*done),
                Some(_) => return Err(Error::invalid("`done` debe ser booleano")),
            };
            let tasks = svc.list_of(owner, None, done).await?;
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
pub struct CompleteTask {
    svc: Arc<TasksService>,
    owner: Uuid,
}

impl CompleteTask {
    pub fn new(svc: Arc<TasksService>, owner: Uuid) -> Self {
        Self { svc, owner }
    }
}

impl Tool for CompleteTask {
    fn name(&self) -> &str {
        "complete_task"
    }

    fn description(&self) -> &str {
        "Marca como hecha una tarea del dueño del proceso."
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
        _ctx: ToolContext,
    ) -> rdd::http::BoxFuture<'static, rdd::Result<CallToolResult>> {
        let svc = Arc::clone(&self.svc);
        let owner = self.owner;
        Box::pin(async move {
            let id = args
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| Error::invalid("falta `id`"))?;
            let id = Uuid::parse_str(id).map_err(|_| Error::invalid("`id` debe ser un UUID"))?;
            let task = svc.update_of(owner, id, None, Some(true)).await?;
            let body = serde_json::to_value(TaskResponse::from(task))
                .map_err(|err| Error::internal(err.into()))?;
            Ok(CallToolResult::json(body))
        })
    }
}
