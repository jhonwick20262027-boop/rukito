//! Punto de entrada de la app (≈ main.ts, §8.4).
//!
//! `#[rdd::main]` carga `.env`, monta `tracing` con `RUST_LOG` y crea el runtime
//! Tokio; el `main` solo describe la app.

// Parte del scaffold todavía no se llama y el proyecto tiene que compilar sin
// avisos (RF-143); no hay carpetas placeholder que silenciar (RF-004).
#![allow(dead_code)]

use std::sync::Arc;
use std::time::Duration;

use rdd::prelude::*;
use rdd_cors::Cors;

use crate::middleware::JwtMiddleware;
use crate::modules::users::Session;

mod app_controller;
mod app_module;
mod app_service;
mod config;
mod generated;
mod middleware;
mod modules;

#[rdd::main]
async fn main() -> Result<()> {
    let config = config::app_config::load();
    let addr = format!("{}:{}", config.host, config.port);

    // El codec se construye aquí, no dentro del módulo: `users::module` recibe el
    // token codec como parámetro para que las pruebas puedan pasar el suyo sin
    // tocar el entorno (§26.3).
    let tokens = modules::users::tokens();

    // `rdd g module` inserta debajo del marcador; no quitar el comentario (§14.3).
    let app = App::new()
        .config(config)
        // El orden importa (RF-080, §26.4): CORS responde el preflight antes de
        // nada, JWT identifica sin rechazar y el rate limit ya tiene identidad.
        .middleware(Cors::from_config().map_err(|err| Error::Internal(err.into()))?)
        .middleware(JwtMiddleware::new(Arc::new(tokens.clone())))
        .middleware(
            RateLimit::new(100, Duration::from_secs(60), rate_key)
                .map_err(|err| Error::Internal(err.into()))?,
        )
        // rdd:modules
        .module(modules::users::module(tokens))
        .module(modules::tasks::module())
        .module(app_module::module());

    // La URL viene solo del entorno (RF-063); el respaldo lo puso
    // `rdd new --database` en `config/database_config.rs` (RF-189).
    let app = app.database(config::database_config::url());

    // `rdd run --memory` deja `RDD_MEMORY=1` para elegir el backend en memoria
    // sin base de datos (§14.1, RF-015).
    let app = if std::env::var("RDD_MEMORY").is_ok() {
        app.in_memory()
    } else {
        app
    };

    app.run(&addr).await
}

/// Una cuenta no comparte cuota con las demás (RF-109). Los anónimos sí comparten
/// cubo: es lo que frena la fuerza bruta contra `/auth/login`, que no tiene sesión.
fn rate_key(req: &Request) -> String {
    match req.ext::<Session>() {
        Some(session) => format!("user:{}", session.id),
        None => "anonymous".to_owned(),
    }
}
