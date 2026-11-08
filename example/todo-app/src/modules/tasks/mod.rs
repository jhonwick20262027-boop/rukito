//! Módulo `tasks` (≈ `tasks.module.ts`, §5).
//!
//! Punto de montaje del dominio: `App::module(modules::tasks::module())`.
//! El controller resuelve `Repo<Task>` del contexto de petición; el adaptador
//! activo lo configura `App`.

pub mod constants;
pub mod controllers;
pub mod dto;
pub mod entities;
pub mod interfaces;
pub mod mappers;
pub mod repositories;
pub mod services;

#[cfg(test)]
mod tests;

use rdd::module;

use self::controllers::tasks_controller::TasksController;

/// El módulo listo para montar (§6.2).
pub fn module() -> rdd::Module {
    module! {
        providers: [],
        controllers: [TasksController],
        middlewares: [],
    }
}
