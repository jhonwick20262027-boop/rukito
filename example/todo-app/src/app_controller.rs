//! Controller raíz (≈ app.controller.ts, §8.2).
//!
//! `GET /` responde JSON (RF-070). `/health` lo añade el núcleo, no este
//! controller, y se puede quitar con `App::without_health` (RF-016).

use rdd::controller;
use rdd::prelude::*;

use crate::app_service::AppService;

pub struct AppController;

#[controller("/")]
impl AppController {
    /// Bienvenida: confirma que la app está montada y respondiendo.
    #[get("/")]
    async fn index() -> Result<String> {
        Ok(AppService::hello())
    }
}
