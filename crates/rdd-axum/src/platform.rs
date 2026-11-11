//! `AxumPlatform`: sirve el `HttpApp` del núcleo sobre Axum (§4.4, RP-003).
//!
//! Registra las rutas de forma **nativa** en el router de Axum —no pasa por
//! `HttpApp::dispatch`— para aprovechar su enrutado y el soporte de capas Tower
//! (RP-011). Todo lo semántico (middlewares, errores, 404/405/413, pánicos) lo
//! hace el núcleo; aquí solo se traduce entre los dos mundos.

use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;

use axum::extract::{Path, Request};
use axum::response::Response;
use axum::routing::{Route, any};
use axum::{Router, routing::MethodRouter};
use rdd_core::error::{PlatformError, PlatformResult};
use rdd_core::http::{
    BoxHandler, HttpApp, Method, Platform, PlatformFuture, RouteDef, ShutdownSignal,
    to_route_syntax,
};
use tokio::net::TcpListener;
use tower::util::BoxCloneSyncService;
use tower::{Layer, Service};

use crate::shim::{RouteEntry, serve_fallback, serve_route};

/// El servicio ya borrado que produce una capa Tower sobre una ruta de `rdd`.
///
/// `BoxCloneSyncService` es lo que Axum usa internamente (`axum::routing::Route`
/// es un alias suyo), así que borrar aquí no añade un segundo nivel de caja.
type ErasedService = BoxCloneSyncService<Request, Response, Infallible>;

/// Capa Tower con el tipo concreto borrado.
///
/// `AxumPlatform` guarda un `Vec` de capas, y un `Vec` exige un único tipo; por
/// eso se borra el servicio en `ErasedService`, que es exactamente lo que
/// `MethodRouter::layer` admite.
#[derive(Clone)]
struct ErasedLayer(Arc<dyn Fn(Route) -> ErasedService + Send + Sync>);

impl Layer<Route> for ErasedLayer {
    type Service = ErasedService;

    fn layer(&self, inner: Route) -> Self::Service {
        (self.0)(inner)
    }
}

/// Plataforma por defecto de `rdd` (RP-012).
#[derive(Clone)]
pub struct AxumPlatform {
    layers: Vec<ErasedLayer>,
}

impl Default for AxumPlatform {
    fn default() -> Self {
        Self::new()
    }
}

impl AxumPlatform {
    /// Plataforma sin capas Tower adicionales.
    pub fn new() -> Self {
        Self { layers: Vec::new() }
    }

    /// Añade una capa Tower al servidor (RP-011).
    ///
    /// Las capacidades del servidor se configuran aquí, nunca en `App`. La capa
    /// queda **por fuera** de la cadena de `rdd` (RP-005): no se inserta entre
    /// sus middlewares ni altera el orden de §11.3.
    pub fn layer<L>(mut self, layer: L) -> Self
    where
        L: Layer<Route> + Clone + Send + Sync + 'static,
        L::Service: Service<Request, Response = Response, Error = Infallible>
            + Clone
            + Send
            + Sync
            + 'static,
        <L::Service as Service<Request>>::Future: Send + 'static,
    {
        self.layers.push(ErasedLayer(Arc::new(move |inner| {
            BoxCloneSyncService::new(layer.clone().layer(inner))
        })));
        self
    }

    /// Construye el router de Axum con las rutas del núcleo (RP-003, RP-008).
    fn router(&self, app: Arc<HttpApp>, shutdown: ShutdownSignal) -> Router {
        let limits = app.limits;
        #[cfg(not(feature = "ws"))]
        let _ = shutdown;
        let fallback = Arc::clone(&app.fallback);
        let fallback_service = Arc::clone(&fallback);
        let fallback_handler = move |req: Request| {
            let fallback = Arc::clone(&fallback_service);
            async move { serve_fallback(fallback, limits, req).await }
        };

        let mut router = Router::new().fallback(fallback_handler);
        for (declared, methods) in group_by_path(&app.routes) {
            let entry = Arc::new(RouteEntry::new(methods, Arc::clone(&fallback), limits));
            let handler_entry = Arc::clone(&entry);
            let handler = move |params: Path<HashMap<String, String>>, req: Request| {
                let entry = Arc::clone(&handler_entry);
                async move { serve_route(entry, params.0, req).await }
            };

            let pattern = to_route_syntax(&declared);
            // RP-008: Axum no fusiona `/tasks/` con `/tasks`; se registran las
            // dos grafías y el shim normaliza antes de entregar al núcleo.
            for spelling in spellings(&pattern) {
                router = router.route(&spelling, apply_layers(any(handler.clone()), &self.layers));
            }
        }

        for upgrade in &app.upgrades {
            let pattern = to_route_syntax(&upgrade.path);
            #[cfg(feature = "ws")]
            let handler = {
                let app = Arc::clone(&app);
                let shutdown = shutdown.clone();
                move |params: Path<HashMap<String, String>>, req: Request| {
                    let app = Arc::clone(&app);
                    let shutdown = shutdown.clone();
                    async move { crate::ws::serve_upgrade(app, shutdown, params.0, req).await }
                }
            };
            #[cfg(not(feature = "ws"))]
            let handler = {
                let app = Arc::clone(&app);
                move |req: Request| {
                    let app = Arc::clone(&app);
                    async move {
                        let resp = app.upgrade_not_implemented(req.uri().path());
                        core_response(resp)
                    }
                }
            };

            for spelling in spellings(&pattern) {
                router = router.route(&spelling, apply_layers(any(handler.clone()), &self.layers));
            }
        }
        router
    }
}

impl Platform for AxumPlatform {
    fn name(&self) -> &'static str {
        "axum"
    }

    fn supports_upgrades(&self) -> bool {
        cfg!(feature = "ws")
    }

    fn serve(
        self: Arc<Self>,
        app: HttpApp,
        listener: TcpListener,
        shutdown: ShutdownSignal,
    ) -> PlatformResult<PlatformFuture<Result<(), PlatformError>>> {
        let grace = app.limits.shutdown_grace;
        let app = Arc::new(app);
        let router = self.router(Arc::clone(&app), shutdown.clone());

        // RP-009: al activarse la señal se dejan de aceptar conexiones nuevas y
        // se drenan las en curso. El plazo `shutdown_grace` se impone por fuera:
        // si vence, se aborta la tarea del servidor.
        let shutdown_for_server = shutdown.clone();
        let server = async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(async move { shutdown_for_server.wait().await })
                .await
        };
        let running = tokio::spawn(server);
        let deadline = async move {
            shutdown.wait().await;
            tokio::time::sleep(grace).await;
        };

        Ok(Box::pin(async move {
            tokio::select! {
                joined = running => match joined {
                    Ok(Ok(())) => Ok(()),
                    Ok(Err(err)) => Err(PlatformError::Server {
                        platform: "axum",
                        message: err.to_string(),
                    }),
                    Err(err) => Err(PlatformError::Server {
                        platform: "axum",
                        message: err.to_string(),
                    }),
                },
                _ = deadline => {
                    tracing::warn!(
                        grace_ms = grace.as_millis(),
                        "el drenaje de axum superó el plazo de apagado: se cierra el servidor"
                    );
                    Ok(())
                }
            }
        }))
    }
}

/// Aplica las capas Tower por fuera de la cadena de `rdd` (RP-005).
fn apply_layers(mut router: MethodRouter, layers: &[ErasedLayer]) -> MethodRouter {
    for layer in layers {
        router = router.layer(layer.clone());
    }
    router
}

/// Respuesta del núcleo al tipo de Axum, sin transportar canales (RP-027).
#[cfg(not(feature = "ws"))]
fn core_response(resp: rdd_core::http::Response) -> Response {
    let (parts, body) = resp.into_parts();
    Response::from_parts(parts, axum::body::Body::from(body))
}

/// Rutas del núcleo agrupadas por ruta declarada, conservando el orden de montaje.
fn group_by_path(routes: &[RouteDef]) -> Vec<(String, HashMap<Method, BoxHandler>)> {
    let mut grouped: Vec<(String, HashMap<Method, BoxHandler>)> = Vec::new();
    for def in routes {
        if let Some((_, methods)) = grouped.iter_mut().find(|(path, _)| path == &def.path) {
            methods.insert(def.method.clone(), Arc::clone(&def.handler));
            continue;
        }
        let mut methods = HashMap::new();
        methods.insert(def.method.clone(), Arc::clone(&def.handler));
        grouped.push((def.path.clone(), methods));
    }
    grouped
}

/// Las dos grafías de una ruta, para cubrir la barra final (RP-008).
fn spellings(pattern: &str) -> Vec<String> {
    if pattern == "/" || pattern.ends_with('/') {
        return vec![pattern.to_owned()];
    }
    vec![pattern.to_owned(), format!("{pattern}/")]
}
