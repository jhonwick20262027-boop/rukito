//! Capa HTTP del núcleo: tipos, extractores, respuestas, rutas, middlewares y
//! plataforma (§4.4, §10, §11).

pub mod extract;
pub mod platform;
pub mod query;
pub mod rate_limit;
pub mod response;
pub mod routes;
pub mod types;

use std::sync::Arc;

pub use extract::{
    Bind, Ctx, Ext, FromContainer, FromRequest, Header, Json, Path, PathParams, Query,
};
pub use platform::{
    DefaultPlatformFn, Platform, PlatformFuture, ShutdownSignal, default_platform,
    set_default_platform,
};
pub use rate_limit::{RateLimit, RateLimitConfigError};
pub use response::{
    AllowedMethods, IntoResponse, body_to_bytes, into_response, json_response, problem_response,
};
pub use routes::{
    HttpApp, Limits, Prepared, RouteDef, invoke_guarded, normalize_path, to_route_syntax,
    with_extensions,
};
pub use types::{
    BoxFuture, BoxHandler, BoxedMiddleware, CoreHandler, FnMiddleware, HandlerFn, Middleware,
    NextArc, NextFn, Request, Response, RouteFn, StatusCode, compose, fn_middleware, materialize,
    run_next,
};

// Re-exportados de la crate `http`, neutrales entre plataformas (§4.3).
pub use http::{Extensions, HeaderMap, HeaderName, HeaderValue, Method, Uri, header};

/// Métodos útiles añadidos a la petición del núcleo (RF-084).
pub trait RequestExt {
    /// Servicio del contenedor (RF-084).
    fn resolve<T: FromContainer>(&self) -> crate::Result<T>;

    /// Valor puesto por un middleware en las extensiones (RF-083).
    fn ext<T: Clone + Send + Sync + 'static>(&self) -> Option<T>;

    /// Parámetros de ruta capturados (RP-003).
    fn params(&self) -> PathParams;
}

impl RequestExt for Request {
    fn resolve<T: FromContainer>(&self) -> crate::Result<T> {
        let container = self
            .extensions()
            .get::<Arc<crate::di::Container>>()
            .cloned()
            .unwrap_or_else(|| Arc::clone(crate::di::default_container()));
        T::from_container(&container)
    }

    fn ext<T: Clone + Send + Sync + 'static>(&self) -> Option<T> {
        self.extensions().get::<T>().cloned()
    }

    fn params(&self) -> PathParams {
        self.extensions()
            .get::<PathParams>()
            .cloned()
            .unwrap_or_default()
    }
}
