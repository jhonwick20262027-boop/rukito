//! Componentes transversales propios del proyecto (§5.1). Lo que es de un solo dominio va en ese módulo, no aquí (RA-004).
//!
//! Cada subcarpeta documenta qué va ahí y qué API de rdd lo cubre (RF-004).

pub mod constants;
pub mod decorators;
pub mod enums;
pub mod exceptions;
pub mod filters;
pub mod guards;
pub mod interceptors;
pub mod interfaces;
pub mod middleware;
pub mod pipes;
pub mod utils;
