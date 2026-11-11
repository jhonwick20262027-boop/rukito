//! `TaskEvents`: el REST de `tasks` alimenta el canal `/ws/tasks` (§21, §26.7).
//!
//! Es un middleware del módulo: envuelve al controller ya autorizado, y solo
//! publica cuando la respuesta es un éxito. Así el contrato REST no cambia y los
//! controllers no saben que existe el tiempo real. El tipo de la tarea es el
//! propio JSON de `TaskResponse`; aquí no se importa el DTO.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use rdd::http::{Method, NextArc};
use rdd::prelude::*;
use uuid::Uuid;

use crate::modules::tasks::TaskFeed;

pub struct TaskEvents;

impl Middleware for TaskEvents {
    fn handle<'a>(
        &'a self,
        req: Request,
        next: NextArc,
    ) -> Pin<Box<dyn Future<Output = Result<Response>> + Send + 'a>> {
        let method = req.method().clone();
        // Los eventos se nombran con la ruta normalizada (RF-018).
        let path = req.uri().path().trim_end_matches('/').to_owned();
        let feed = req
            .extensions()
            .get::<Arc<Container>>()
            .and_then(|container| container.get::<TaskFeed>());

        Box::pin(async move {
            let resp = next(req).await?;
            let Some(feed) = feed else {
                return Ok(resp);
            };
            if !resp.status().is_success() {
                return Ok(resp);
            }

            let id = path
                .strip_prefix("/tasks/")
                .and_then(|rest| Uuid::parse_str(rest).ok());
            if method == Method::POST && path == "/tasks" {
                if let Ok(task) = serde_json::from_slice(resp.body()) {
                    feed.created(task).await;
                }
            } else if (method == Method::PUT || method == Method::PATCH) && id.is_some() {
                if let Ok(task) = serde_json::from_slice(resp.body()) {
                    feed.updated(task).await;
                }
            } else if method == Method::DELETE
                && let Some(id) = id
            {
                feed.deleted(id).await;
            }
            Ok(resp)
        })
    }
}
