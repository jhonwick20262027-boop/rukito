//! Extractor que resuelve el repositorio de tareas desde el adaptador de la app.

use std::future::Future;

use rdd::http::{Bind, Ctx, FromRequest};
use rdd::prelude::*;

use crate::modules::tasks::entities::task_entity::Task;

pub struct TasksRepository(pub Repo<Task>);

impl FromRequest for TasksRepository {
    fn from_request(ctx: &Ctx, _bind: Bind) -> impl Future<Output = Result<Self, Error>> + Send {
        async move { Ok(Self(ctx.resolve::<Repo<Task>>()?)) }
    }
}
