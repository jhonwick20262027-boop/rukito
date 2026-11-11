//! Aceptación del demo (§26.6) y de la plataforma (§21, RF-007).
//!
//! Un test de integración no puede importar el binario, así que el árbol de
//! `src/` se incluye por ruta: `crate::modules::…`, `crate::middleware::…` y
//! `crate::config::…` resuelven igual que en `main.rs` y no hay que duplicar el
//! cableado.
//!
//! Las comprobaciones 1 y 7 de §26.6 (`rdd migrate` y persistencia tras reiniciar)
//! necesitan un proceso y un archivo SQLite de verdad: se verifican con el CLI,
//! no aquí. Las otras seis sí son automáticas.

#![allow(dead_code)]

#[path = "../src/app_controller.rs"]
mod app_controller;
#[path = "../src/app_module.rs"]
mod app_module;
#[path = "../src/app_service.rs"]
mod app_service;
#[path = "../src/config/mod.rs"]
mod config;
#[path = "../src/middleware/mod.rs"]
mod middleware;
#[path = "../src/modules/mod.rs"]
mod modules;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use rdd::AxumPlatform;
use rdd::http::{Method, Platform, ShutdownSignal, header, into_response};
use rdd::prelude::*;
use rdd::{controller, module};
use rdd_cors::Cors;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use crate::middleware::JwtMiddleware;
use crate::modules::tasks::TaskFeed;
use crate::modules::users::domain::{PasswordHasher, User};
use crate::modules::users::infrastructure::argon2_hasher::Argon2Hasher;
use crate::modules::users::{JwtTokens, Role, Session, TokenCodec};

/// Secreto de prueba. El del entorno no se toca: `std::env::set_var` es `unsafe`
/// en edition 2024 y el workspace prohíbe `unsafe`, así que el codec se inyecta.
const SECRET: &str = "secreto-de-prueba-lo-bastante-largo-para-hs256";
const ORIGIN: &str = "http://localhost:5173";
const PASSWORD: &str = "contrasena-de-prueba";

/// El mismo cableado que `main.rs` (§26.4), con el backend en memoria compartido
/// para poder sembrar cuentas sin pasar por `/auth/register`.
struct Demo {
    http: HttpApp,
    backend: Arc<MemoryBackend>,
    feed: TaskFeed,
}

fn demo(limit: u32) -> Demo {
    let backend = Arc::new(MemoryBackend::new());
    let tokens = JwtTokens::new(SECRET, Duration::from_secs(3600));
    let feed = TaskFeed::new();
    let http = App::new()
        .middleware(cors())
        .middleware(JwtMiddleware::new(Arc::new(tokens.clone())))
        .middleware(
            RateLimit::new(limit, Duration::from_secs(60), rate_key)
                .expect("el límite de prueba es válido"),
        )
        .provide(feed.clone())
        .socket("/ws/tasks", feed.clone())
        .module(modules::users::module(tokens))
        .module(modules::tasks::module())
        .module(app_module::module())
        .adapter(backend.clone())
        .build()
        .expect("la app debe construirse")
        .http;
    Demo {
        http,
        backend,
        feed,
    }
}

/// La misma sección `[cors]` del `rdd.toml` del demo, escrita aquí para que la
/// suite no dependa del directorio desde el que se lance `cargo test`.
fn cors() -> Cors {
    Cors::new()
        .allow_origin(ORIGIN)
        .allow_methods(["GET", "POST", "PUT", "PATCH", "DELETE"])
        .allow_headers(["content-type", "authorization"])
        .max_age(Duration::from_secs(3600))
        .build()
        .expect("la configuración CORS de prueba es válida")
}

fn rate_key(req: &Request) -> String {
    match req.ext::<Session>() {
        Some(session) => format!("user:{}", session.id),
        None => "anonymous".to_owned(),
    }
}

/// Siembra una cuenta y devuelve su JWT. Emitirlo con el codec en vez de llamar a
/// `/auth/login` mantiene libre el cubo de los anónimos para las pruebas de 429.
async fn seed(demo: &Demo, email: &str, role: Role) -> String {
    let hash = Argon2Hasher.hash(PASSWORD).expect("argon2 debe hashear");
    let user = User::new(email.to_owned(), hash, role).expect("el correo de prueba es válido");
    let token = JwtTokens::new(SECRET, Duration::from_secs(3600))
        .issue(&user.session())
        .expect("el codec debe emitir");
    Repo::<User>::new(demo.backend.clone())
        .save(&user)
        .await
        .expect("sembrar la cuenta");
    token
}

fn request(method: Method, uri: &str, body: &str) -> Request {
    http::Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .body(bytes::Bytes::copy_from_slice(body.as_bytes()))
        .expect("petición válida")
}

fn with_token(mut req: Request, token: &str) -> Request {
    req.headers_mut().insert(
        header::AUTHORIZATION,
        format!("Bearer {token}").parse().expect("cabecera válida"),
    );
    req
}

async fn send(http: &HttpApp, method: Method, uri: &str, body: &str) -> Response {
    http.dispatch(request(method, uri, body)).await
}

async fn send_as(demo: &Demo, method: Method, uri: &str, body: &str, token: &str) -> Response {
    demo.http
        .dispatch(with_token(request(method, uri, body), token))
        .await
}

fn credentials(email: &str) -> String {
    format!(r#"{{"email":"{email}","password":"{PASSWORD}"}}"#)
}

fn json(resp: &Response) -> serde_json::Value {
    serde_json::from_slice(resp.body().as_ref()).expect("el cuerpo debe ser JSON")
}

/// §10.3: toda respuesta de error es `application/problem+json`.
fn problem(resp: &Response) -> serde_json::Value {
    assert_eq!(
        resp.headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("application/problem+json"),
    );
    json(resp)
}

#[tokio::test]
async fn health_responde_ok() {
    let demo = demo(100);
    let resp = send(&demo.http, Method::GET, "/health", "").await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(json(&resp)["status"], "ok");
}

#[tokio::test]
async fn la_raiz_responde_json() {
    let demo = demo(100);
    let resp = send(&demo.http, Method::GET, "/", "").await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("application/json"),
    );
}

#[tokio::test]
async fn ruta_inexistente_es_404_en_problem_details() {
    let demo = demo(100);
    let resp = send(&demo.http, Method::GET, "/no-existe", "").await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body = problem(&resp);
    assert_eq!(body["status"], 404);
    assert_eq!(body["instance"], "/no-existe");
}

#[tokio::test]
async fn metodo_no_permitido_es_405_con_allow() {
    let demo = demo(100);
    let resp = send(&demo.http, Method::POST, "/", "").await;
    assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert!(
        resp.headers()
            .get(header::ALLOW)
            .is_some_and(|value| value.to_str().unwrap().contains("GET")),
        "falta la cabecera Allow (RF-072)"
    );
    assert_eq!(problem(&resp)["status"], 405);
}

#[test]
fn todos_los_errores_salen_en_problem_details() {
    let cases = [
        (Error::bad_request("mal formado"), 400),
        (Error::Unauthorized, 401),
        (Error::Forbidden, 403),
        (Error::not_found("no existe"), 404),
        (Error::conflict("ya existe"), 409),
        (Error::invalid("no procesable"), 422),
    ];
    for (err, status) in cases {
        let resp = into_response(StatusCode::OK, Err::<(), _>(err));
        assert_eq!(resp.status().as_u16(), status);
        let body = problem(&resp);
        assert_eq!(body["type"], "about:blank");
        assert_eq!(body["status"], status);
        assert!(body["title"].is_string());
        assert!(body["detail"].is_string());
    }
}

/// Controller de prueba: verifica que un panic no tumba la app (RF-073).
struct BoomController;

#[controller("/boom")]
impl BoomController {
    #[get("/")]
    async fn boom() -> Result<()> {
        panic!("fallo provocado para verificar RF-073");
    }
}

#[tokio::test]
async fn un_panico_es_500_y_no_filtra_el_detalle() {
    let http = App::new()
        .module(module! { controllers: [BoomController], middlewares: [] })
        .build()
        .expect("la app debe construirse")
        .http;
    let resp = send(&http, Method::GET, "/boom", "").await;
    assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = problem(&resp);
    assert_eq!(body["status"], 500);
    // RF-041: el detalle interno va al log, no al cliente.
    assert!(
        !body["detail"]
            .as_str()
            .unwrap_or_default()
            .contains("provocado"),
        "el 500 no debe exponer el mensaje del panic"
    );
}

/// §26.6, comprobación 2: registro 201 y login 200 con un JWT utilizable.
#[tokio::test]
async fn registro_y_login_devuelven_un_jwt() {
    let demo = demo(100);

    let resp = send(
        &demo.http,
        Method::POST,
        "/auth/register",
        &credentials("ana@example.com"),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body = json(&resp);
    assert_eq!(body["email"], "ana@example.com");
    // Sin `ADMIN_EMAIL` en el entorno nadie nace admin (§26.3), y el cliente no
    // puede elegir el rol porque `RegisterDto` no lleva ese campo.
    assert_eq!(body["role"], "user");
    assert!(
        body.get("password_hash").is_none(),
        "la respuesta no debe serializar el hash"
    );

    let resp = send(
        &demo.http,
        Method::POST,
        "/auth/login",
        &credentials("ana@example.com"),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json(&resp);
    assert_eq!(body["token_type"], "Bearer");
    let token = body["access_token"]
        .as_str()
        .expect("el login debe devolver un JWT")
        .to_owned();
    assert_eq!(token.split('.').count(), 3, "un JWT tiene tres partes");

    let resp = send_as(&demo, Method::GET, "/users/me", "", &token).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(json(&resp)["email"], "ana@example.com");

    let resp = send(
        &demo.http,
        Method::POST,
        "/auth/login",
        r#"{"email":"ana@example.com","password":"otra-cosa"}"#,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(problem(&resp)["status"], 401);
}

/// §26.6, comprobación 3: sin token 401, con token 200.
#[tokio::test]
async fn las_tareas_exigen_sesion() {
    let demo = demo(100);

    let resp = send(&demo.http, Method::GET, "/tasks", "").await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(problem(&resp)["status"], 401);

    // `JwtMiddleware` identifica sin rechazar: un token inservible deja la
    // petición sin sesión y es `RequireAuth` quien responde 401 (§26.3).
    let resp = send_as(&demo, Method::GET, "/tasks", "", "token-inservible").await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    let token = seed(&demo, "ana@example.com", Role::User).await;
    let resp = send_as(&demo, Method::GET, "/tasks", "", &token).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(json(&resp).as_array().unwrap().len(), 0);
}

/// §26.6, comprobación 4: `DELETE` exige `admin`. El rol se comprueba antes que
/// la propiedad, así que un `user` recibe 403 aunque la tarea no sea suya.
#[tokio::test]
async fn borrar_exige_rol_admin() {
    let demo = demo(100);
    let admin = seed(&demo, "admin@example.com", Role::Admin).await;
    let user = seed(&demo, "ana@example.com", Role::User).await;

    let resp = send_as(
        &demo,
        Method::POST,
        "/tasks",
        r#"{"title":"del admin"}"#,
        &admin,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let id = json(&resp)["id"].as_str().unwrap().to_owned();

    let resp = send_as(&demo, Method::DELETE, &format!("/tasks/{id}"), "", &user).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    assert_eq!(problem(&resp)["status"], 403);

    let resp = send_as(&demo, Method::DELETE, &format!("/tasks/{id}"), "", &admin).await;
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    assert!(resp.body().is_empty(), "204 no lleva cuerpo (RF-053)");
}

/// §26.6, comprobación 5 (C-09): CORS responde el preflight antes que JWT y antes
/// que el rate limit. Si hubiera seguido la cadena, `/tasks` no acepta OPTIONS y
/// la respuesta sería 405; y llevaría cabeceras `RateLimit-*`.
#[tokio::test]
async fn el_preflight_no_pasa_por_autenticacion() {
    let demo = demo(100);
    let resp = demo
        .http
        .dispatch(
            http::Request::builder()
                .method(Method::OPTIONS)
                .uri("/tasks")
                .header(header::ORIGIN, ORIGIN)
                .header("access-control-request-method", "DELETE")
                .header("access-control-request-headers", "authorization")
                .body(bytes::Bytes::new())
                .expect("petición válida"),
        )
        .await;

    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    assert!(resp.body().is_empty());
    assert_eq!(resp.headers()["access-control-allow-origin"], ORIGIN);
    assert!(
        resp.headers()["access-control-allow-methods"]
            .to_str()
            .unwrap()
            .contains("DELETE")
    );
    assert!(
        resp.headers()["access-control-allow-headers"]
            .to_str()
            .unwrap()
            .contains("authorization")
    );
    assert_eq!(resp.headers()["access-control-max-age"], "3600");
    assert!(
        resp.headers()["vary"]
            .to_str()
            .unwrap()
            .to_ascii_lowercase()
            .contains("origin"),
        "RF-096 exige Vary en el preflight"
    );
    assert!(
        !resp.headers().contains_key("ratelimit-limit"),
        "el preflight no debe llegar al rate limit"
    );
}

/// RF-098: un origen no configurado recibe 204 sin ninguna `Access-Control-*`.
/// El bloqueo lo ejecuta el navegador, no el servidor.
#[tokio::test]
async fn un_origen_no_permitido_no_recibe_cabeceras_cors() {
    let demo = demo(100);
    let resp = demo
        .http
        .dispatch(
            http::Request::builder()
                .method(Method::OPTIONS)
                .uri("/tasks")
                .header(header::ORIGIN, "http://malicioso.example")
                .header("access-control-request-method", "GET")
                .body(bytes::Bytes::new())
                .expect("petición válida"),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    assert!(!resp.headers().contains_key("access-control-allow-origin"));
    assert!(
        resp.headers()["vary"]
            .to_str()
            .unwrap()
            .to_ascii_lowercase()
            .contains("origin")
    );
}

/// §26.6, comprobación 6 (RF-107): superar el límite es 429 con `Retry-After` y
/// las tres cabeceras `RateLimit-*`, en Problem Details.
#[tokio::test]
async fn superar_el_limite_es_429() {
    let demo = demo(2);
    for _ in 0..2 {
        let resp = send(&demo.http, Method::GET, "/health", "").await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(resp.headers()["ratelimit-limit"], "2");
    }
    let resp = send(&demo.http, Method::GET, "/health", "").await;
    assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(resp.headers()["ratelimit-remaining"], "0");
    assert!(resp.headers().contains_key("ratelimit-reset"));
    assert!(
        resp.headers()["retry-after"]
            .to_str()
            .unwrap()
            .parse::<u64>()
            .is_ok(),
        "Retry-After debe ser un número de segundos"
    );
    assert_eq!(problem(&resp)["status"], 429);
}

/// RF-109: la cuota es por identidad, no por aplicación. Agotar la de una cuenta
/// no deja fuera a la otra.
#[tokio::test]
async fn la_cuota_es_por_cuenta() {
    let demo = demo(2);
    let ana = seed(&demo, "ana@example.com", Role::User).await;
    let bob = seed(&demo, "bob@example.com", Role::User).await;

    for _ in 0..2 {
        let resp = send_as(&demo, Method::GET, "/tasks", "", &ana).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }
    let resp = send_as(&demo, Method::GET, "/tasks", "", &ana).await;
    assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);

    let resp = send_as(&demo, Method::GET, "/tasks", "", &bob).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.headers()["ratelimit-remaining"], "1");
}

/// §26.6, comprobación 8: el acceso cruzado responde 404, no 403, para no revelar
/// que el recurso existe. Vale también para un admin (§26.5).
#[tokio::test]
async fn las_tareas_ajenas_son_404_no_403() {
    let demo = demo(100);
    let ana = seed(&demo, "ana@example.com", Role::User).await;
    let bob = seed(&demo, "bob@example.com", Role::User).await;
    let admin = seed(&demo, "admin@example.com", Role::Admin).await;

    let resp = send_as(&demo, Method::POST, "/tasks", r#"{"title":"de ana"}"#, &ana).await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let id = json(&resp)["id"].as_str().unwrap().to_owned();
    let uri = format!("/tasks/{id}");

    let resp = send_as(&demo, Method::GET, &uri, "", &bob).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    assert_eq!(problem(&resp)["status"], 404);

    let resp = send_as(&demo, Method::PUT, &uri, r#"{"done":true}"#, &bob).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    let resp = send_as(&demo, Method::GET, "/tasks", "", &bob).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(
        json(&resp).as_array().unwrap().is_empty(),
        "el listado tampoco debe incluir tareas ajenas"
    );

    // El admin pasa `RequireRole` pero no es el dueño: 404 igual que todos.
    let resp = send_as(&demo, Method::DELETE, &uri, "", &admin).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    let resp = send_as(&demo, Method::GET, &uri, "", &ana).await;
    assert_eq!(resp.status(), StatusCode::OK, "la tarea de ana sigue ahí");
}

#[tokio::test]
async fn crud_del_modulo_de_ejemplo() {
    let demo = demo(100);
    // `DELETE` exige `admin` (§26.4): el CRUD completo se hace con esa identidad.
    let token = seed(&demo, "admin@example.com", Role::Admin).await;

    let resp = send_as(&demo, Method::GET, "/users/me", "", &token).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let me = json(&resp)["id"].as_str().unwrap().to_owned();

    let resp = send_as(&demo, Method::POST, "/tasks", r#"{"title":"pan"}"#, &token).await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body = json(&resp);
    let id = body["id"]
        .as_str()
        .expect("el id debe ser un UUID en texto")
        .to_owned();
    uuid::Uuid::parse_str(&id).expect("el id debe ser un UUID válido (§7.2)");
    assert_eq!(
        body["owner_id"].as_str().unwrap(),
        me,
        "la tarea nace con dueño (§26.5)"
    );

    let resp = send_as(&demo, Method::GET, &format!("/tasks/{id}"), "", &token).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(json(&resp)["title"], "pan");

    let resp = send_as(&demo, Method::GET, "/tasks?q=pan", "", &token).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(json(&resp).as_array().unwrap().len(), 1);

    let resp = send_as(
        &demo,
        Method::PUT,
        &format!("/tasks/{id}"),
        r#"{"done":true}"#,
        &token,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(json(&resp)["done"], true);

    let resp = send_as(&demo, Method::DELETE, &format!("/tasks/{id}"), "", &token).await;
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    assert!(resp.body().is_empty(), "204 no lleva cuerpo (RF-053)");

    let resp = send_as(&demo, Method::GET, &format!("/tasks/{id}"), "", &token).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    assert_eq!(problem(&resp)["status"], 404);
}

#[tokio::test]
async fn validacion_del_modulo_de_ejemplo() {
    use crate::modules::tasks::domain::task::MAX_TITLE_LEN;

    let demo = demo(100);
    let token = seed(&demo, "ana@example.com", Role::User).await;

    let resp = send_as(&demo, Method::POST, "/tasks", r#"{"title":"  "}"#, &token).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    assert_eq!(problem(&resp)["instance"], "/tasks");

    let largo = format!(r#"{{"title":"{}"}}"#, "x".repeat(MAX_TITLE_LEN + 1));
    let resp = send_as(&demo, Method::POST, "/tasks", &largo, &token).await;
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(problem(&resp)["status"], 422);

    // RF-055: un parámetro de ruta que no convierte es 400.
    let resp = send_as(&demo, Method::GET, "/tasks/abc", "", &token).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    assert_eq!(problem(&resp)["status"], 400);
}

/// Aceptación de la fase 0.8 (§21): dos clientes, 401 sin token y 1001 al apagar.
mod canal {
    use super::*;

    type Client = WebSocketStream<MaybeTlsStream<TcpStream>>;

    const DRAIN: Duration = Duration::from_secs(15);

    fn ws_request(addr: SocketAddr, token: Option<&str>) -> http::Request<()> {
        let mut request = format!("ws://{addr}/ws/tasks")
            .into_client_request()
            .expect("la petición WS debe ser válida");
        if let Some(token) = token {
            request.headers_mut().insert(
                header::AUTHORIZATION,
                format!("Bearer {token}").parse().expect("cabecera válida"),
            );
        }
        request
    }

    async fn next(ws: &mut Client) -> Message {
        tokio::time::timeout(Duration::from_secs(10), ws.next())
            .await
            .unwrap_or_else(|_| panic!("el canal no envió nada en 10s"))
            .unwrap_or_else(|| panic!("el canal se cerró sin mensaje"))
            .unwrap_or_else(|err| panic!("error de trama: {err}"))
    }

    async fn next_close(ws: &mut Client) -> u16 {
        for _ in 0..64 {
            if let Message::Close(frame) = next(ws).await {
                return frame.map(|frame| u16::from(frame.code)).unwrap_or(1000);
            }
        }
        panic!("no llegó un cierre");
    }

    /// POST con `Connection: close`: la respuesta se lee completa hasta EOF.
    async fn post_task(addr: SocketAddr, token: &str, title: &str) -> String {
        let body = format!("{{\"title\":\"{title}\"}}");
        let request = format!(
            "POST /tasks HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer {token}\r\n\
             Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let mut stream = TcpStream::connect(addr).await.expect("conexión HTTP");
        stream.write_all(request.as_bytes()).await.expect("envío");
        let mut response = String::new();
        stream.read_to_string(&mut response).await.expect("lectura");
        response
    }

    #[tokio::test]
    async fn dos_clientes_reciben_el_mismo_evento_y_sin_token_no_hay_101() {
        let demo = demo(100);
        let token = seed(&demo, "canal@example.com", Role::User).await;

        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("puerto efímero");
        listener
            .set_nonblocking(true)
            .expect("listener no bloqueante");
        let addr = listener.local_addr().expect("socket vinculado");
        let listener = TcpListener::from_std(listener).expect("listener de Tokio");
        let shutdown = ShutdownSignal::new();
        let future = Arc::new(AxumPlatform::new())
            .serve(demo.http, listener, shutdown.clone())
            .expect("la plataforma debe arrancar");
        let server = tokio::spawn(async move {
            if let Err(err) = future.await {
                panic!("la plataforma terminó con error: {err}");
            }
        });

        // Sin token: el middleware no identifica y `authorize` rechaza antes del 101.
        match tokio_tungstenite::connect_async(ws_request(addr, None)).await {
            Ok((_, response)) => panic!("sin token no debe haber 101, llegó {}", response.status()),
            Err(tokio_tungstenite::tungstenite::Error::Http(response)) => {
                assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
                assert_eq!(
                    response
                        .headers()
                        .get(header::CONTENT_TYPE)
                        .and_then(|value| value.to_str().ok()),
                    Some("application/problem+json"),
                    "el rechazo es Problem Details (RF-198)"
                );
            }
            Err(err) => panic!("error inesperado del handshake: {err}"),
        }

        let (mut a, response_a) = tokio_tungstenite::connect_async(ws_request(addr, Some(&token)))
            .await
            .expect("el cliente A debe conectar");
        let (mut b, response_b) = tokio_tungstenite::connect_async(ws_request(addr, Some(&token)))
            .await
            .expect("el cliente B debe conectar");
        assert_eq!(response_a.status(), 101);
        assert_eq!(response_b.status(), 101);
        let _ready_a = next(&mut a).await;
        let _ready_b = next(&mut b).await;

        let response = post_task(addr, &token, "pan").await;
        assert!(
            response.contains("201"),
            "la tarea debe crearse: {response}"
        );

        let mut events = Vec::new();
        for ws in [&mut a, &mut b] {
            match next(ws).await {
                Message::Text(text) => events.push(text.to_string()),
                other => panic!("se esperaba el evento del canal, llegó {other:?}"),
            }
        }
        assert_eq!(
            events[0], events[1],
            "ambos clientes reciben el mismo evento"
        );
        assert!(events[0].contains("\"type\":\"created\""), "{}", events[0]);
        assert!(events[0].contains("\"title\":\"pan\""), "{}", events[0]);

        shutdown.trigger();
        assert_eq!(next_close(&mut a).await, 1001, "apagado ordenado del canal");
        assert_eq!(next_close(&mut b).await, 1001, "apagado ordenado del canal");
        drop((a, b));
        tokio::time::timeout(DRAIN, server)
            .await
            .expect("la plataforma no cerró dentro del plazo")
            .expect("la tarea del servidor entró en pánico");
    }
}
