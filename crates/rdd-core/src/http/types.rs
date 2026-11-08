//! Tipos HTTP neutrales del núcleo (RP-002) y contratos de handler y middleware.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

pub type Method = http::Method;
pub type StatusCode = http::StatusCode;
pub type HeaderMap = http::HeaderMap;
pub type Uri = http::Uri;
pub type HeaderName = http::HeaderName;
pub type HeaderValue = http::HeaderValue;
pub use http::header;

/// Petición del núcleo: cuerpo ya leído en memoria, con límite aplicado (RP-002, ADR-12).
pub type Request = http::Request<bytes::Bytes>;

/// Respuesta del núcleo.
pub type Response = http::Response<bytes::Bytes>;

/// Caja de futuro homogénea usada por handlers, middlewares y plataformas.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Handler interno del núcleo: cadena de middlewares más handler, devolviendo
/// `Result` para que un middleware pueda cortocuitar con error (RF-082).
pub type CoreHandler =
    Arc<dyn Fn(Request) -> BoxFuture<'static, crate::Result<Response>> + Send + Sync>;

/// Handler ya envuelto por su cadena completa y con los errores materializados:
/// **nunca** devuelve `Err` a la plataforma (RP-004).
pub type BoxHandler = Arc<dyn Fn(Request) -> BoxFuture<'static, Response> + Send + Sync>;

/// El `next` que recibe un middleware: el resto de la cadena.
pub type NextArc = CoreHandler;

/// Sinónimos usados por la tabla de rutas y por `RouteDef`.
pub type HandlerFn = CoreHandler;
pub type RouteFn = CoreHandler;

/// Trait de middleware del núcleo (§11.1). No depende de Tower (ADR-02).
pub trait Middleware: Send + Sync + 'static {
    /// Envuelve la cadena restante. El primer registrado es el más externo (§11.3).
    fn handle<'a>(
        &'a self,
        req: Request,
        next: NextArc,
    ) -> Pin<Box<dyn Future<Output = crate::Result<Response>> + Send + 'a>>;
}

/// Envoltorio público del trait para registrarlo en `App`, `Module` o un controller.
pub type BoxedMiddleware = Arc<dyn Middleware>;

/// Continúa la cadena desde un middleware.
pub async fn run_next(next: NextArc, req: Request) -> crate::Result<Response> {
    next(req).await
}

/// Construye un middleware a partir de una función asíncrona (§11.2 B).
///
/// El closure debe ser `Fn + Send + Sync` y devuelve `Result<Response>`; puede
/// llamar a `next.run(req)` para continuar o cortocircuitar (RF-082).
pub fn fn_middleware<F, Fut>(f: F) -> impl Middleware
where
    F: Fn(Request, NextFn) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = crate::Result<Response>> + Send + 'static,
{
    FnMiddleware { f }
}

/// El `next` que ve un middleware creado con [`fn_middleware`].
pub struct NextFn(NextArc);

impl NextFn {
    /// Continúa la cadena.
    pub async fn run(self, req: Request) -> crate::Result<Response> {
        (self.0)(req).await
    }
}

pub struct FnMiddleware<F> {
    f: F,
}

impl<F, Fut> Middleware for FnMiddleware<F>
where
    F: Fn(Request, NextFn) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = crate::Result<Response>> + Send + 'static,
{
    fn handle<'a>(
        &'a self,
        req: Request,
        next: NextArc,
    ) -> Pin<Box<dyn Future<Output = crate::Result<Response>> + Send + 'a>> {
        Box::pin((self.f)(req, NextFn(next)))
    }
}

/// Compone la cadena completa: `mws[0]` queda más externo que `mws[1]` (§11.3).
pub fn compose(mws: &[Arc<dyn Middleware>], handler: CoreHandler) -> CoreHandler {
    let mut current: CoreHandler = handler;
    for mw in mws.iter().rev() {
        let mw = Arc::clone(mw);
        let next: CoreHandler = current;
        current = Arc::new(move |req: Request| {
            let mw = Arc::clone(&mw);
            let next: CoreHandler = Arc::clone(&next);
            Box::pin(async move { mw.handle(req, next).await })
        });
    }
    current
}

/// Reabre un handler ya materializado como handler de núcleo (RF-080).
///
/// Las `RouteDef` que emite `#[controller]` llegan cerradas (RP-004), pero el
/// ensamblado todavía tiene que envolverlas con los middlewares de módulo y
/// globales, y esos aceptan cortocircuitar con `Err`. Un handler cerrado nunca
/// devuelve error, así que el `Ok` es firme.
pub fn reopen(handler: BoxHandler) -> CoreHandler {
    Arc::new(move |req: Request| {
        let handler = Arc::clone(&handler);
        Box::pin(async move { Ok(handler(req).await) })
    })
}

/// Materializa los errores de una cadena como Problem Details, con la ruta de la
/// petición como `instance` (§10.3). Es el último paso antes de entregar la ruta
/// a la plataforma, que no debe ver nunca un `Err` (RP-004).
pub fn materialize(chain: CoreHandler) -> BoxHandler {
    Arc::new(move |req: Request| {
        let chain = Arc::clone(&chain);
        Box::pin(async move {
            let instance = req.uri().path().to_owned();
            match chain(req).await {
                Ok(resp) => resp,
                Err(err) => crate::http::response::error_response_for(&err, &instance),
            }
        })
    })
}
