//! Servicios técnicos: pool, caché, colas, almacenamiento (§5.1). Los repositories de cada módulo hablan con esta carpeta, nunca al revés.
//!
//! Cada subcarpeta documenta qué va ahí y qué API de rdd lo cubre (RF-004).

pub mod cache;
pub mod database;
pub mod external_services;
pub mod messaging;
pub mod queue;
pub mod storage;
