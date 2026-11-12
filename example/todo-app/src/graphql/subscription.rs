//! Raíz de suscripciones (§10.9, §26.9).
//!
//! El hub emite a todos los dueños; el filtro por dueño vive aquí (§26.5).

use rdd::Payload;
use rdd::graphql::{GraphqlContext, GraphqlStream, graphql_object};
use rdd::prelude::*;

use crate::modules::tasks::{GraphqlTask, TaskFeed, TaskResponse};
use crate::modules::users::Session;

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
