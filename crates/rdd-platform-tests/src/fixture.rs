//! La app de prueba de la suite (§4.5).
//!
//! Ambas sondas —la de red y `dispatch`— ven **exactamente** estas rutas, que es
//! lo que hace comparable la conformidad. No usa macros: este crate depende de
//! `rdd-core`, no de la fachada (RA-010), y las macros generan rutas `::rdd::…`
//! (RA-013).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use rdd_core::api::{Router, route_def};
use rdd_core::http::{Bind, Ctx, HeaderValue, HttpApp, Json, Method, Middleware, StatusCode};
use rdd_core::http::{FromRequest, fn_middleware};
use rdd_core::{App, Controller};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Límite de cuerpo de la app de prueba, en bytes (C-05).
pub(crate) const BODY_LIMIT: usize = 64;

/// Retardo de la ruta lenta, base del drenaje que verifica C-11.
pub(crate) const SLOW_DELAY: Duration = Duration::from_millis(250);

/// Plazo de apagado: holgado para que C-11 distinga «drenó» de «cortó».
const SHUTDOWN_GRACE: Duration = Duration::from_secs(5);

/// Veces que `/counted` llegó de verdad al handler.
///
/// Es `static` porque `Controller::register` no admite estado por instancia. Lo
/// lee solo `c05`, y cada binario de prueba corre `run_all` una vez: no hay dos
/// plataformas contando sobre el mismo valor a la vez.
static COUNTED_CALLS: AtomicUsize = AtomicUsize::new(0);

/// Restablece el contador antes de una corrida.
pub(crate) fn reset_counter() {
    COUNTED_CALLS.store(0, Ordering::SeqCst);
}

/// Veces que `/counted` ejecutó el handler desde el último reinicio.
pub(crate) fn counted_calls() -> usize {
    COUNTED_CALLS.load(Ordering::SeqCst)
}

#[derive(Serialize)]
struct Task {
    id: u32,
    title: String,
}

#[derive(Deserialize)]
struct NewTask {
    title: String,
}

#[derive(Serialize)]
struct Echo {
    id: String,
    title: String,
}

/// Handler de C-07: siempre entra en pánico.
///
/// Es una `fn` y no un closure para fijar el `Output` del futuro: un closure
/// que solo paniquea tiene tipo `!` y `route_def` no puede inferir `T`.
async fn boom(_: Ctx) -> rdd_core::Result<Task> {
    panic!("pánico controlado de la suite (C-07)")
}

/// Controller de la suite: cubre los casos C-02 a C-08, C-11 y C-12.
struct Suite;

impl Controller for Suite {
    const NAME: &'static str = "suite";

    fn register(router: &mut Router) {
        // C-03, C-08, C-12: ruta lista, sin parámetros.
        router.push(route_def(
            Method::GET,
            "/tasks",
            StatusCode::OK,
            Vec::new(),
            |_: Ctx| async move {
                Ok(vec![Task {
                    id: 1,
                    title: "pan".to_owned(),
                }])
            },
        ));

        // C-05, C-06: cuerpo JSON; inválido → 400.
        router.push(route_def(
            Method::POST,
            "/tasks",
            StatusCode::CREATED,
            Vec::new(),
            |ctx: Ctx| async move {
                let input = Json::<NewTask>::from_request(&ctx, Bind::none())
                    .await?
                    .into_inner();
                Ok(Task {
                    id: 2,
                    title: input.title,
                })
            },
        ));

        // C-04: parámetro de ruta con UUID; inválido → 400.
        router.push(route_def(
            Method::GET,
            "/tasks/:id",
            StatusCode::OK,
            Vec::new(),
            |ctx: Ctx| async move {
                let id: Uuid = ctx.params().parse("id")?;
                Ok(Echo {
                    id: id.to_string(),
                    title: "leido".to_owned(),
                })
            },
        ));

        // C-04: parámetro en el medio de la ruta.
        router.push(route_def(
            Method::POST,
            "/tasks/:id/complete",
            StatusCode::OK,
            Vec::new(),
            |ctx: Ctx| async move {
                let id: Uuid = ctx.params().parse("id")?;
                Ok(Echo {
                    id: id.to_string(),
                    title: "completado".to_owned(),
                })
            },
        ));

        // C-07: pánico dentro del handler → 500 y el servidor sigue vivo.
        router.push(route_def(
            Method::GET,
            "/boom",
            StatusCode::OK,
            Vec::new(),
            boom,
        ));

        // C-11: petición en curso durante el apagado.
        router.push(route_def(
            Method::GET,
            "/slow",
            StatusCode::OK,
            Vec::new(),
            |_: Ctx| async move {
                tokio::time::sleep(SLOW_DELAY).await;
                Ok(Task {
                    id: 3,
                    title: "lento".to_owned(),
                })
            },
        ));

        // C-05: ruta real cuyo handler NO debe ejecutarse con un cuerpo grande.
        router.push(route_def(
            Method::POST,
            "/counted",
            StatusCode::OK,
            Vec::new(),
            |_: Ctx| async move {
                COUNTED_CALLS.fetch_add(1, Ordering::SeqCst);
                Ok(Task {
                    id: 4,
                    title: "ejecutado".to_owned(),
                })
            },
        ));
    }
}

/// Middleware global que marca toda respuesta, incluidas 404 y 405 (C-08, RF-081).
fn marker() -> impl Middleware {
    fn_middleware(|req, next| async move {
        let mut resp = next.run(req).await?;
        resp.headers_mut()
            .insert("x-suite", HeaderValue::from_static("presente"));
        Ok(resp)
    })
}

/// La app HTTP de la suite, ya compuesta y lista para cualquier plataforma.
pub(crate) fn http_app() -> HttpApp {
    App::new()
        .without_health()
        .body_limit(BODY_LIMIT)
        .shutdown_grace(SHUTDOWN_GRACE)
        .middleware(marker())
        .controller::<Suite>()
        .build()
        .expect("la app de la suite debe construirse")
        .http
}
