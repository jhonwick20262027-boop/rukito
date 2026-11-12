//! Raíces del esquema GraphQL del demo (§10.9, §26.9).
//!
//! Es la raíz de composición: el único sitio que puede nombrar usuarios y tareas
//! a la vez (RA-004). Cada raíz vive en su archivo y la macro publica sus campos;
//! los tipos de entrada y salida los publica el módulo dueño.

mod mutation;
mod query;
mod subscription;

pub use self::mutation::Mutation;
pub use self::query::Query;
pub use self::subscription::Subscription;
