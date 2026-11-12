//! Contrato GraphQL de `tasks` (§10.9, §26.9).
//!
//! Vive junto a `TaskResponse` y no duplica la regla de negocio: publica solo lo
//! que un cliente GraphQL necesita ver, sin `owner_id`, y la entrada de
//! `createTask` que la raíz de composición consume.

use rdd::{GraphqlInput, GraphqlObject};
use uuid::Uuid;

use super::dto::TaskResponse;

/// Tarea tal como la ve un cliente GraphQL; `owner_id` no se publica.
#[derive(GraphqlObject)]
pub struct Task {
    pub id: Uuid,
    pub title: String,
    pub done: bool,
}

/// Entrada de `createTask`; el resto de los campos los fija el caso de uso.
#[derive(GraphqlInput)]
pub struct CreateTaskInput {
    pub title: String,
}

impl From<TaskResponse> for Task {
    fn from(item: TaskResponse) -> Self {
        Self {
            id: item.id,
            title: item.title,
            done: item.done,
        }
    }
}
