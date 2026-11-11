//! Modelo y puertos del dominio `tasks` (§5.1, RA-001).

pub mod task;
pub mod tasks_repository;

pub use task::{Task, TaskError};
pub use tasks_repository::TaskRepository;
