//! API de registro de rutas, la forma que emite `#[controller]` (§8.2, §8.5).
//!
//! La macro construye una [`RouteDef`] por handler mediante [`route_def`]; este
//! módulo es el único consumidor de esa forma, así que macro y núcleo comparten
//! un contrato estable y documentado.

use std::future::Future;
use std::sync::Arc;

use crate::http::extract::{Bind, Ctx, FromRequest};
use crate::http::response::IntoResponse;
use crate::http::routes::RouteDef;
use crate::http::types::{HandlerFn, Method, Middleware, Request, StatusCode, compose};

/// Construye la `RouteDef` que emite la macro para un handler (expansión §8.5).
///
/// `F` recibe un [`Ctx`] propietario y devuelve el `Result` del handler; la
/// extracción de parámetros ocurre dentro del closure generado, por lo que no
/// hay límite de aridad impuesto por el sistema de tipos (RF-057).
///
/// `middlewares` son los declarados con `#[middleware]` **en la ruta**, ya en el
/// orden de §11.3; los del controller los añade [`crate::module`] por fuera.
pub fn route_def<T, F, Fut>(
    method: Method,
    path: &str,
    status: StatusCode,
    middlewares: Vec<Arc<dyn Middleware>>,
    f: F,
) -> RouteDef
where
    T: IntoResponse + Send + 'static,
    F: Fn(Ctx) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = crate::Result<T>> + Send + 'static,
{
    let f = Arc::new(f);
    let handler: HandlerFn = Arc::new(move |req: Request| {
        let f = Arc::clone(&f);
        Box::pin(async move {
            (f)(Ctx::of(req))
                .await
                .map(|value| value.into_response(status))
        })
    });
    RouteDef::from_chain(method, path, compose(&middlewares, handler))
}

/// Extrae un parámetro del contexto con el enlace que pasa la macro (RF-057).
pub async fn extract<T: FromRequest>(ctx: &Ctx, bind: Bind) -> crate::Result<T> {
    T::from_request(ctx, bind).await
}

/// Estado base según el verbo, cuando el handler no devuelve `()` (RF-053).
pub fn default_status(method: &Method) -> StatusCode {
    match *method {
        Method::POST => StatusCode::CREATED,
        _ => StatusCode::OK,
    }
}

/// Registrador acumulado por `Controller::register` al montar un controller.
#[derive(Default)]
pub struct Router {
    routes: Vec<RouteDef>,
    controller: &'static str,
}

impl Router {
    /// Registrador vacío para un controller.
    pub fn new(controller: &'static str) -> Self {
        Self {
            routes: Vec::new(),
            controller,
        }
    }

    /// Añade una ruta. Dos rutas con el mismo verbo y path es un error de
    /// programa que la macro ya impide (RDD0304); aquí se registra y gana la
    /// primera, nunca se paniquea en el arranque.
    pub fn push(&mut self, route: RouteDef) -> &mut Self {
        if self
            .routes
            .iter()
            .any(|r| r.method == route.method && r.path == route.path)
        {
            tracing::error!(
                route = %format!("{} {}", route.method, route.path),
                controller = self.controller,
                "ruta duplicada al montar (RDD0304)"
            );
            return self;
        }
        self.routes.push(route);
        self
    }

    /// Rutas acumuladas.
    pub fn routes(&self) -> &[RouteDef] {
        &self.routes
    }

    /// Desmonta el registrador: es lo que devuelve el `routes()` generado (RF-056).
    pub fn into_defs(self) -> Vec<RouteDef> {
        self.routes
    }
}
