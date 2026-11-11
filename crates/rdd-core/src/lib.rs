//! # rdd-core
//!
//! Núcleo del framework REST DDD: `App`, sistema de middlewares propio, errores
//! en Problem Details, la tabla de rutas con [`HttpApp::dispatch`], el puerto de
//! persistencia [`DbAdapter`] y la interfaz [`Platform`](http::Platform) que
//! separa el núcleo de cualquier servidor HTTP.
//!
//! Este crate **no** conoce Axum ni hyper (§4.4, RA-011) ni ningún driver de base
//! de datos (§12, RA-014). Un proyecto de usuario depende de la fachada `rdd`, no
//! de `rdd-core` directamente (RA-013).

pub mod api;
pub mod app;
pub mod db;
pub mod di;
pub mod domain;
pub mod error;
pub mod http;
pub mod module;

pub use app::{App, AppConfig, Backend, BuiltApp, DEFAULT_BODY_LIMIT, NO_DEFAULT_PLATFORM};
pub use db::{Db, DbAdapter, MemoryBackend, Persist, Repo, Row, Value};
pub use domain::{Aggregate, DomainEvent};
pub use error::{ContainerError, Error, PlatformError, PlatformResult, Problem, Result};
pub use http::rate_limit::{RateLimit, RateLimitConfigError};
pub use module::{Controller, Module, ModuleBuilder, ModuleMember, ProviderMarker};

/// Tipos que un proyecto de usuario importa normalmente.
pub mod prelude {
    pub use crate::api::{Router, extract, route_def};
    pub use crate::app::{App, AppConfig, Backend};
    pub use crate::db::{Db, DbAdapter, MemoryBackend, Persist, Repo, Row, Value};
    pub use crate::di::{Container, ContainerBuilder, ProviderReg};
    pub use crate::domain::Aggregate;
    pub use crate::error::{Error, Problem, Result};
    pub use crate::http::{
        AllowedMethods, Bind, Ctx, Ext, FromContainer, FromRequest, Header, HttpApp, Json, Limits,
        Middleware, Path, PathParams, Platform, Query, RateLimit, RateLimitConfigError, Request,
        RequestExt, Response, RouteDef, ShutdownSignal, StatusCode, compose, fn_middleware, header,
    };
    pub use crate::module::{Controller, Module, ModuleBuilder, ModuleMember, ProviderMarker};
}
