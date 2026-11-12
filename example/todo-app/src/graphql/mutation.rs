//! Raíz de mutaciones (§10.9, §26.9).
//!
//! La entrada la publica el módulo `tasks`: así el contrato GraphQL de la tarea
//! vive junto a su DTO y la raíz solo orquesta el caso de uso.

use rdd::graphql::{GraphqlContext, graphql_object};
use rdd::prelude::*;
use uuid::Uuid;

use crate::modules::tasks::{CreateTaskInput, GraphqlTask, TaskResponse, TasksService};
use crate::modules::users::Session;

#[derive(Default)]
pub struct Mutation;

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
