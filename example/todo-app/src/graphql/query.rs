//! Raíz de consultas (§10.9, §26.9).
//!
//! Los resolutores entran por los casos de uso, nunca por los DTOs HTTP, y la
//! identidad la dejan los globales en el contexto.

use rdd::graphql::{GraphqlContext, graphql_object};
use rdd::prelude::*;

use crate::modules::tasks::{GraphqlTask, TaskResponse, TasksService};
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
