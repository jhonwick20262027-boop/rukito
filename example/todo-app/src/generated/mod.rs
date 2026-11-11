//! Código que genera `rdd db generate` (§13.3). NO se edita a mano.
//!
//! En 0.4 Prisma produce aquí los tipos de fila y sus `impl Persist` (§12.2) a
//! partir de `prisma/schema.prisma`. Este snippet es documental: muestra la forma
//! exacta de lo que va a llegar, y está marcado `ignore` porque el generador
//! todavía no existe (RF-004, RF-006).
//!
//! Mientras tanto, el agregado del módulo de ejemplo implementa `Aggregate` y
//! `Persist` a mano en `modules/tasks/domain/task.rs` (§14.2). Cuando el
//! generador entre, ese impl se sustituye por el de aquí y el dominio se queda
//! solo con las invariantes.
//!
//! ```rust,ignore
//! // Lo emitirá `rdd db generate` (RF-133: determinista, mismo esquema → mismo archivo).
//! #[derive(Debug, Clone, serde::Serialize)]
//! pub struct TaskRow {
//!     pub id: uuid::Uuid,
//!     pub title: String,
//!     pub done: bool,
//! }
//!
//! impl rdd::Aggregate for TaskRow {
//!     type Id = uuid::Uuid;
//!
//!     fn id(&self) -> &Self::Id {
//!         &self.id
//!     }
//! }
//!
//! impl rdd::Persist for TaskRow {
//!     const TABLE: &'static str = "tasks";
//!     const COLUMNS: &'static [&'static str] = &["id", "title", "done"];
//!
//!     fn to_values(&self) -> Vec<rdd::Value> {
//!         vec![
//!             rdd::Value::Uuid(self.id),
//!             rdd::Value::from(self.title.clone()),
//!             rdd::Value::Bool(self.done),
//!         ]
//!     }
//!
//!     fn from_row(row: &rdd::Row) -> rdd::Result<Self> {
//!         // Por nombre y no por posición: el orden de `Row` no es contractual (§12.1).
//!         Ok(Self {
//!             id: row.value("id")?.as_uuid()?,
//!             title: row.value("title")?.as_text()?.to_owned(),
//!             done: row.value("done")?.as_bool()?,
//!         })
//!     }
//! }
//! ```
