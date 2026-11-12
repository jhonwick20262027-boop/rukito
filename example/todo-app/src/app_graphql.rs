//! Raíces del esquema GraphQL del demo (§10.9, §26.9).
//!
//! Es la raíz de composición: el único sitio que puede nombrar usuarios y tareas
//! a la vez (RA-004). Los resolutores entran por los casos de uso, nunca por los
//! DTOs HTTP, y la identidad la dejan los globales en el contexto.

use rdd::Payload;
use rdd::graphql::{GraphqlContext, GraphqlInput, GraphqlStream, graphql_object};
use rdd::prelude::*;
use uuid::Uuid;

use crate::modules::tasks::{GraphqlTask, TaskFeed, TaskResponse, TasksService};
use crate::modules::users::Session;

#[derive(Default)]
pub struct Query;

#[graphql_object(query)]
impl Query {
    /// Tareas del dueño del token.
    async fn tasks(&self, ctx: &GraphqlContext) -> Result<Vec<GraphqlTask>> {
        let session = ctx.ext::<Session>().ok_or(Error::Unauthorized)?;
        let svc = ctx.resolve::<TasksService>()?;
        let tasks = svc.list_of(session.id, None, None).await?;
        Ok(tasks
            .into_iter()
            .map(|task| GraphqlTask::from(TaskResponse::from(task)))
            .collect())
    }
}

#[derive(Default)]
pub struct Mutation;

#[derive(GraphqlInput)]
pub struct CreateTaskInput {
    pub title: String,
}

#[graphql_object(mutation)]
impl Mutation {
    async fn create_task(
        &self,
        ctx: &GraphqlContext,
        input: CreateTaskInput,
    ) -> Result<GraphqlTask> {
        let session = ctx.ext::<Session>().ok_or(Error::Unauthorized)?;
        let svc = ctx.resolve::<TasksService>()?;
        let task = svc.create(session.id, input.title, None).await?;
        Ok(TaskResponse::from(task).into())
    }

    async fn complete_task(&self, ctx: &GraphqlContext, id: Uuid) -> Result<GraphqlTask> {
        let session = ctx.ext::<Session>().ok_or(Error::Unauthorized)?;
        let svc = ctx.resolve::<TasksService>()?;
        let task = svc.update_of(session.id, id, None, Some(true)).await?;
        Ok(TaskResponse::from(task).into())
    }
}

#[derive(Default)]
pub struct Subscription;

#[graphql_object(subscription)]
impl Subscription {
    /// Cambios de las tareas del dueño del token, uno por evento del hub.
    async fn task_changed(&self, ctx: &GraphqlContext) -> Result<GraphqlStream<GraphqlTask>> {
        let session = ctx.ext::<Session>().ok_or(Error::Unauthorized)?;
        let feed = ctx
            .get::<TaskFeed>()
            .ok_or_else(|| Error::internal(anyhow::anyhow!("falta TaskFeed")))?;
        let (tx, rx) = tokio::sync::mpsc::channel(64);
        let mut events = feed.subscribe();
        tokio::spawn(async move {
            while let Ok(Payload::Json(event)) = events.recv().await {
                let Some(task) = event.get("task") else {
                    continue;
                };
                let Ok(task) = serde_json::from_value::<TaskResponse>(task.clone()) else {
                    continue;
                };
                // El hub es de todos: el filtro por dueño va aquí (§26.5).
                if task.owner_id != session.id {
                    continue;
                }
                if tx.send(GraphqlTask::from(task)).await.is_err() {
                    break; // el cliente desconectó (RF-238)
                }
            }
        });
        Ok(GraphqlStream::new(rx))
    }
}
