//! Punto de entrada de la app (≈ main.ts, §8.4).
//!
//! `#[rdd::main]` carga `.env`, monta `tracing` con `RUST_LOG` y crea el runtime
//! Tokio; el `main` solo describe la app.

// El scaffold reserva carpetas para fases posteriores (§5, RF-004): parte de su
// contenido todavía no se llama y el proyecto tiene que compilar sin avisos.
#![allow(dead_code)]

use rdd::prelude::*;

mod app_controller;
mod app_module;
mod app_service;
mod common;
mod config;
mod generated;
mod infrastructure;
mod integrations;
mod modules;
mod shared;

#[rdd::main]
async fn main() -> Result<()> {
    let config = config::app_config::load();
    let addr = format!("{}:{}", config.host, config.port);
    let database_url =
        config::database_config::url().unwrap_or_else(|| "sqlite://todo_app.db".to_owned());

    // `rdd g module` inserta debajo del marcador; no quitar el comentario (§14.3).
    let app = App::new()
        .config(config)
        // rdd:modules
        .module(modules::tasks::module())
        .module(app_module::module());

    // Las migraciones se ejecutan explícitamente con `rdd migrate` antes del
    // despliegue; el proceso web solo conecta al adaptador configurado.
    let app = if std::env::var_os("RDD_MEMORY").is_some() {
        app.in_memory()
    } else {
        app.database(database_url)
    };

    app.run(&addr).await
}
