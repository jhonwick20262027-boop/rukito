//! # rdd-platform-tests
//!
//! Suite de conformidad de plataformas (§4.5, RP-020 a RP-022).
//!
//! Una abstracción con un solo adaptador tiende a copiar la forma de ese
//! adaptador. Esta crate impide eso: **toda plataforma pasa los mismos casos**
//! contra el mismo `HttpApp`, levantada en un puerto efímero y atacada con HTTP
//! real. Los casos de esta fase son C-02 a C-08, C-11 y C-12; C-01, C-09 y C-10
//! llegan con la fase 0.3, cuando existan middlewares anidados, CORS y
//! `x-request-id`.
//!
//! ```ignore
//! #[tokio::test]
//! async fn conformance() {
//!     rdd_platform_tests::run_all(|| rdd_axum::AxumPlatform::new()).await;
//! }
//! ```
//!
//! La plataforma de referencia del núcleo también se verifica (RP-021) con
//! [`run_all_dispatch`], que corre los mismos casos sobre
//! [`rdd_core::http::HttpApp::dispatch`] sin pasar por la red.

mod cases;
mod fixture;
mod probe;

use std::sync::Arc;
use std::time::Duration;

use probe::Probe;
use rdd_core::http::{Platform, ShutdownSignal};
use tokio::net::TcpListener;

/// Margen de espera por el cierre ordenado de la plataforma.
const DRAIN_TIMEOUT: Duration = Duration::from_secs(15);

/// Corre la suite contra la plataforma que produce `make`.
///
/// Levanta la app de la fixture en `127.0.0.1:0`, la sirve con la plataforma
/// indicada y ejecuta los casos con peticiones HTTP reales (§4.5). Al terminar
/// verifica que el servidor haya cerrado sin errores.
pub async fn run_all<P, M>(make: M)
where
    P: Platform,
    M: Fn() -> P,
{
    let app = fixture::http_app();
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("no se pudo vincular el puerto efímero de pruebas");
    let addr = listener
        .local_addr()
        .expect("el socket de pruebas debe estar vinculado");

    let platform = Arc::new(make());
    let name = platform.name();
    let shutdown = ShutdownSignal::new();
    let serving = platform
        .serve(app, listener, shutdown.clone())
        .unwrap_or_else(|err| panic!("la plataforma `{name}` no arrancó: {err}"));
    let server = tokio::spawn(serving);

    cases::run_all_cases(&Probe::Http {
        addr,
        shutdown: shutdown.clone(),
    })
    .await;

    // C-11 ya activó el apagado; si por algún camino no llegó a activarse, se
    // activa aquí para no dejar el servidor colgado.
    shutdown.trigger();
    match tokio::time::timeout(DRAIN_TIMEOUT, server).await {
        Err(_) => panic!("la plataforma no cerró dentro de {DRAIN_TIMEOUT:?} tras el apagado"),
        Ok(Err(err)) => panic!("la tarea del servidor entró en pánico: {err}"),
        Ok(Ok(Err(err))) => panic!("la plataforma terminó con error: {err}"),
        Ok(Ok(Ok(()))) => {}
    }
}

/// Corre la suite contra [`HttpApp::dispatch`] (RP-021, plataforma de referencia).
///
/// Los mismos casos, sin socket: si algo diverge entre las dos sondas, el fallo
/// es del núcleo o de la plataforma, nunca del test.
pub async fn run_all_dispatch() {
    let app = Arc::new(fixture::http_app());
    cases::run_all_cases(&Probe::InProcess(app)).await;
}
