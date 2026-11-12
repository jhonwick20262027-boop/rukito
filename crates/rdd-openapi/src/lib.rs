//! Documento OpenAPI 3.1 y su interfaz embebida (§10.10).
//!
//! El crate es neutral: modela el documento, lo valida contra las rutas que el
//! `HttpApp` montó y sirve la UI como HTML autocontenido. El montaje en rutas
//! HTTP lo aporta `rdd-axum` (RP-032), igual que `/graphql` y `/mcp`.
//!
//! ```no_run
//! use rdd_openapi::{OpenApiBuilder, Operation, Server, Tag};
//!
//! let doc = OpenApiBuilder::new("todo-app", "0.1.0")
//!     .description("Demo de rdd")
//!     .server(Server::new("http://localhost:3000").description("Local"))
//!     .tag(Tag::new("tasks").description("Tareas del dueño del token"))
//!     .operation(
//!         "get",
//!         "/tasks",
//!         Operation::new("listTasks", "Lista las tareas del token")
//!             .tag("tasks")
//!             .bearer()
//!             .json_response(200, "Tareas", serde_json::json!({ "type": "array" })),
//!     )
//!     .build();
//!
//! doc.validate()?;
//! # Ok::<(), anyhow::Error>(())
//! ```

mod builder;
mod http;
mod model;

pub use builder::OpenApiBuilder;
pub use http::{OpenApiHttp, OpenApiHttpConfig, OpenApiHttpResponse};
pub use model::{
    ApiResponse, Components, Info, MediaType, OpenApi, Operation, Parameter, ParameterLocation,
    PathItem, RequestBody, Server, Tag,
};

/// Ruta por defecto de la documentación (§10.10).
pub const DEFAULT_PATH: &str = "/api-docs";

/// Alias por defecto: `/docs` y `/swagger` sirven la misma UI.
pub const DEFAULT_ALIASES: [&str; 2] = ["/docs", "/swagger"];

/// Esquema de seguridad que `Operation::bearer` referencia.
pub const BEARER_SCHEME: &str = "bearerAuth";

use std::collections::BTreeMap;

/// Requisito de seguridad: `[{"bearerAuth": []}]`.
pub type SecurityRequirement = BTreeMap<String, Vec<String>>;
