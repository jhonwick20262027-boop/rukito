//! Canal de tiempo real de `tasks` (§10.4, §10.5, §26.7).
//!
//! El REST sigue siendo el camino de escritura y el contrato no cambia: el
//! middleware `TaskEvents` observa el resultado de las rutas y lo publica en el
//! hub. El canal solo exige identidad; quién puede cambiar una tarea lo sigue
//! decidiendo la capa HTTP.

use rdd::http::BoxFuture;
use rdd::prelude::*;
use rdd::{Hub, Lagged, Payload};
use uuid::Uuid;

use crate::modules::users::Session;

/// Sala de los eventos de tareas; el nombre es el contrato del canal.
pub const TOPIC: &str = "tasks";

/// Difunde los cambios de `tasks` a los clientes conectados a `/ws/tasks`.
#[derive(Clone)]
pub struct TaskFeed {
    hub: Hub,
}

impl Default for TaskFeed {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskFeed {
    /// Crea el hub local del canal (RF-203: sin historial ni persistencia).
    pub fn new() -> Self {
        Self { hub: Hub::new() }
    }

    /// La tarea ya viene en la forma que devuelve `TaskResponse`.
    pub async fn created(&self, task: serde_json::Value) {
        self.publish(serde_json::json!({ "type": "created", "task": task }))
            .await;
    }

    pub async fn updated(&self, task: serde_json::Value) {
        self.publish(serde_json::json!({ "type": "updated", "task": task }))
            .await;
    }

    pub async fn deleted(&self, id: Uuid) {
        self.publish(serde_json::json!({ "type": "deleted", "id": id }))
            .await;
    }

    async fn publish(&self, event: serde_json::Value) {
        self.hub.broadcast(TOPIC, event).await;
    }
}

impl SocketHandler for TaskFeed {
    /// Sin sesión no hay 101: el handshake responde 401 (RF-198, §26.7).
    fn authorize(&self, ctx: &SocketContext) -> Result<()> {
        ctx.ext::<Session>().map(|_| ()).ok_or(Error::Unauthorized)
    }

    fn handle(
        &self,
        _ctx: SocketContext,
        mut conn: Box<dyn SocketConnection>,
    ) -> BoxFuture<'static, Result<()>> {
        let hub = self.hub.clone();
        Box::pin(async move {
            let mut sub = hub.subscribe(TOPIC);
            conn.send(SocketMessage::Text(
                serde_json::json!({ "type": "ready", "topic": TOPIC }).to_string(),
            ))
            .await?;

            loop {
                tokio::select! {
                    incoming = conn.recv() => match incoming {
                        Some(Ok(SocketMessage::Close(_, _))) | None => break,
                        Some(Ok(_)) => {}
                        Some(Err(err)) => return Err(err),
                    },
                    event = sub.recv() => match event {
                        Ok(Payload::Text(text)) => {
                            conn.send(SocketMessage::Text(text)).await?;
                        }
                        Ok(Payload::Json(value)) => {
                            conn.send(SocketMessage::Text(value.to_string())).await?;
                        }
                        Ok(Payload::Binary(bytes)) => {
                            conn.send(SocketMessage::Binary(bytes)).await?;
                        }
                        // RF-196: el suscriptor atrasado no pierde eventos en
                        // silencio; se cierra con 1013 como en §10.4.
                        Err(Lagged(_)) => {
                            conn.close(1013, "cliente lento".to_owned()).await;
                            return Ok(());
                        }
                    },
                }
            }
            Ok(())
        })
    }
}
