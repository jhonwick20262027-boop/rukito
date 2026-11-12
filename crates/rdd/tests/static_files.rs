//! S-01 a S-07 (§20, §10.11): archivos estáticos con `AxumPlatform`.
//!
//! Cada caso levanta la plataforma real en `127.0.0.1:0`, habla HTTP/1.1 por
//! hyper y apaga con `ShutdownSignal`. El directorio se crea con `tempfile` en
//! cada prueba, así que no depende del árbol del repositorio.

#![cfg(feature = "static-files")]

use std::io::Write as _;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper_util::rt::TokioIo;
use rdd::http::{
    HeaderMap, Middleware, NextFn, Platform, Request, RequestExt, ShutdownSignal, StatusCode,
    fn_middleware, header,
};
use rdd::{App, AxumPlatform, Error, StaticFiles, StaticFilesConfig};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::timeout;

const WAIT: Duration = Duration::from_secs(10);

// --- identidad y app -----------------------------------------------------

#[derive(Clone)]
struct Identity;

fn seed_identity() -> impl Middleware {
    fn_middleware(|mut req: Request, next: NextFn| async move {
        if req
            .headers()
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            == Some("Bearer ana")
        {
            req.extensions_mut().insert(Identity);
        }
        next.run(req).await
    })
}

fn require_identity(req: &Request) -> rdd::Result<()> {
    req.ext::<Identity>().map(|_| ()).ok_or(Error::Unauthorized)
}

fn http_app() -> rdd::http::HttpApp {
    App::new()
        .without_health()
        .middleware(seed_identity())
        .build()
        .expect("la app de prueba debe construirse")
        .http
}

// --- directorio de prueba ------------------------------------------------

fn site() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    write(dir.path(), "index.html", "<h1>todo-app</h1>");
    write(dir.path(), "assets/app.js", "console.log(1);");
    write(dir.path(), "secret.txt", "no");
    write(dir.path(), ".env", "secreto");
    dir
}

fn write(root: &Path, relative: &str, content: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().expect("padre")).expect("mkdir");
    let mut file = std::fs::File::create(path).expect("archivo");
    file.write_all(content.as_bytes()).expect("escribir");
}

fn files(dir: &Path, config: StaticFilesConfig) -> StaticFiles {
    StaticFiles::new(dir)
        .expect("raíz estática")
        .config(config)
        .expect("configuración estática")
}

// --- servidor y cliente --------------------------------------------------

struct Running {
    addr: std::net::SocketAddr,
    shutdown: ShutdownSignal,
    task: tokio::task::JoinHandle<rdd::error::PlatformResult<()>>,
}

impl Running {
    async fn stop(self) {
        self.shutdown.trigger();
        match timeout(WAIT, self.task).await {
            Err(_) => panic!("la plataforma no cerró dentro de {WAIT:?}"),
            Ok(Err(err)) => panic!("la tarea del servidor entró en pánico: {err}"),
            Ok(Ok(Err(err))) => panic!("la plataforma terminó con error: {err}"),
            Ok(Ok(Ok(()))) => {}
        }
    }
}

async fn start(files: StaticFiles) -> Running {
    start_app(http_app(), files).await
}

async fn start_app(app: rdd::http::HttpApp, files: StaticFiles) -> Running {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("no se pudo vincular el puerto efímero");
    let addr = listener.local_addr().expect("socket sin dirección");
    let shutdown = ShutdownSignal::new();
    let serving = Arc::new(AxumPlatform::new().static_files(files))
        .serve(app, listener, shutdown.clone())
        .unwrap_or_else(|err| panic!("el montaje estático no arrancó: {err}"));
    let task = tokio::spawn(serving);
    Running {
        addr,
        shutdown,
        task,
    }
}

struct Reply {
    status: StatusCode,
    headers: HeaderMap,
    body: Bytes,
}

async fn send(addr: std::net::SocketAddr, req: http::Request<Full<Bytes>>) -> Reply {
    let stream = TcpStream::connect(addr)
        .await
        .unwrap_or_else(|err| panic!("no se pudo conectar a {addr}: {err}"));
    let (mut sender, conn) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
        .await
        .expect("falló el handshake HTTP/1.1");
    tokio::spawn(async move {
        let _ = conn.await;
    });
    let resp = sender
        .send_request(req)
        .await
        .unwrap_or_else(|err| panic!("la petición no obtuvo respuesta: {err}"));
    let (parts, body) = resp.into_parts();
    let body = timeout(WAIT, body.collect())
        .await
        .expect("la respuesta no terminó a tiempo")
        .expect("no se pudo leer el cuerpo")
        .to_bytes();
    Reply {
        status: parts.status,
        headers: parts.headers,
        body,
    }
}

fn get(uri: &str) -> http::Request<Full<Bytes>> {
    request("GET", uri, &[])
}

fn request<'a>(
    method: &str,
    uri: &str,
    extra: &[(&'a str, &'a str)],
) -> http::Request<Full<Bytes>> {
    let mut builder = http::Request::builder().method(method).uri(uri);
    for (name, value) in extra {
        builder = builder.header(*name, *value);
    }
    builder
        .body(Full::new(Bytes::new()))
        .expect("petición de prueba")
}

// --- casos ---------------------------------------------------------------

#[tokio::test]
async fn s01_sirve_el_indice_del_montaje() {
    let dir = site();
    let running = start(files(dir.path(), StaticFilesConfig::new().mount("/site"))).await;

    let reply = send(running.addr, get("/site")).await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(
        reply.headers[header::CONTENT_TYPE],
        "text/html; charset=utf-8"
    );
    assert_eq!(reply.headers[header::CACHE_CONTROL], "no-cache");
    assert_eq!(reply.headers["x-content-type-options"], "nosniff");
    assert!(reply.headers.contains_key(header::ETAG));
    assert_eq!(reply.body.as_ref(), b"<h1>todo-app</h1>");

    running.stop().await;
}

#[tokio::test]
async fn s01_las_rutas_del_nucleo_ganan_al_montaje_raiz() {
    let dir = site();
    let app = App::new().build().expect("app con /health").http;
    let running = start_app(app, files(dir.path(), StaticFilesConfig::new().mount("/"))).await;

    let health = send(running.addr, get("/health")).await;
    assert_eq!(health.status, StatusCode::OK);
    assert_eq!(health.body.as_ref(), br#"{"status":"ok"}"#);

    let index = send(running.addr, get("/")).await;
    assert_eq!(index.body.as_ref(), b"<h1>todo-app</h1>");

    running.stop().await;
}

#[tokio::test]
async fn s02_asset_con_mime_inmutable_y_head_sin_cuerpo() {
    let dir = site();
    let running = start(files(dir.path(), StaticFilesConfig::new().mount("/site"))).await;

    let reply = send(running.addr, get("/site/assets/app.js")).await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(
        reply.headers[header::CONTENT_TYPE],
        "text/javascript; charset=utf-8"
    );
    assert_eq!(
        reply.headers[header::CACHE_CONTROL],
        "public, max-age=31536000, immutable"
    );

    let head = send(running.addr, request("HEAD", "/site/assets/app.js", &[])).await;
    assert_eq!(head.status, StatusCode::OK);
    assert_eq!(head.headers[header::CONTENT_LENGTH], "15");
    assert!(head.body.is_empty());

    running.stop().await;
}

#[tokio::test]
async fn s03_etag_repetido_responde_304() {
    let dir = site();
    let running = start(files(dir.path(), StaticFilesConfig::new().mount("/site"))).await;

    let first = send(running.addr, get("/site/index.html")).await;
    let etag = first.headers[header::ETAG]
        .to_str()
        .expect("etag")
        .to_owned();
    let second = send(
        running.addr,
        request(
            "GET",
            "/site/index.html",
            &[(header::IF_NONE_MATCH.as_str(), &etag)],
        ),
    )
    .await;
    assert_eq!(second.status, StatusCode::NOT_MODIFIED);
    assert_eq!(second.headers[header::ETAG], etag);
    assert!(second.body.is_empty());

    running.stop().await;
}

#[tokio::test]
async fn s04_traversal_y_dotfiles_no_salen_de_la_raiz() {
    let dir = site();
    let running = start(files(dir.path(), StaticFilesConfig::new().mount("/site"))).await;

    for uri in [
        "/site/../secret.txt",
        "/site/%2e%2e/secret.txt",
        "/site/assets/%2e%2e/secret.txt",
        "/site/assets\\..\\secret.txt",
        "/site/.env",
    ] {
        let reply = send(running.addr, get(uri)).await;
        assert_eq!(reply.status, StatusCode::NOT_FOUND, "{uri}");
        assert!(
            !String::from_utf8_lossy(&reply.body).contains("secreto"),
            "{uri} no debe filtrar el contenido del archivo"
        );
    }

    // El servidor sigue vivo después de los intentos.
    assert_eq!(
        send(running.addr, get("/site/index.html")).await.status,
        StatusCode::OK
    );

    running.stop().await;
}

#[tokio::test]
async fn s05_spa_solo_para_html_sin_extension() {
    let dir = site();
    let running = start(files(
        dir.path(),
        StaticFilesConfig::new().mount("/site").spa(true),
    ))
    .await;

    let dashboard = send(
        running.addr,
        request(
            "GET",
            "/site/dashboard",
            &[(header::ACCEPT.as_str(), "text/html")],
        ),
    )
    .await;
    assert_eq!(dashboard.status, StatusCode::OK);
    assert_eq!(dashboard.body.as_ref(), b"<h1>todo-app</h1>");

    let missing = send(running.addr, get("/site/missing.js")).await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);

    let no_html = send(running.addr, get("/site/dashboard")).await;
    assert_eq!(no_html.status, StatusCode::NOT_FOUND);

    running.stop().await;
}

#[tokio::test]
async fn s06_solo_get_y_head_y_404_canonico() {
    let dir = site();
    let running = start(files(dir.path(), StaticFilesConfig::new().mount("/site"))).await;

    let post = send(running.addr, request("POST", "/site/index.html", &[])).await;
    assert_eq!(post.status, StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(post.headers[header::ALLOW], "GET, HEAD");

    let missing = send(running.addr, get("/site/no-existe")).await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);
    assert_eq!(
        missing.headers[header::CONTENT_TYPE],
        "application/problem+json"
    );

    running.stop().await;
}

#[tokio::test]
async fn s07_authorize_corre_despues_de_la_cadena_global() {
    let dir = site();
    let running = start(files(
        dir.path(),
        StaticFilesConfig::new()
            .mount("/site")
            .public(false)
            .authorize(require_identity),
    ))
    .await;

    let anonymous = send(running.addr, get("/site/index.html")).await;
    assert_eq!(anonymous.status, StatusCode::UNAUTHORIZED);

    let authorized = send(
        running.addr,
        request(
            "GET",
            "/site/index.html",
            &[(header::AUTHORIZATION.as_str(), "Bearer ana")],
        ),
    )
    .await;
    assert_eq!(authorized.status, StatusCode::OK);

    running.stop().await;
}

#[tokio::test]
async fn s07_montajes_invalidos_no_arrancan() {
    let dir = site();

    // Choque exacto con la ruta `/health` del núcleo.
    let collision = StaticFiles::new(dir.path())
        .and_then(|files| files.config(StaticFilesConfig::new().mount("/health")))
        .expect("montaje construible");
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("puerto efímero");
    let app = App::new().build().expect("app").http;
    let err = Arc::new(AxumPlatform::new().static_files(collision))
        .serve(app, listener, ShutdownSignal::new())
        .err()
        .expect("el choque con /health debe impedir arrancar");
    assert!(
        err.to_string().contains("colisiona"),
        "mensaje inesperado: {err}"
    );

    // Dos montajes que se solapan.
    let one = files(dir.path(), StaticFilesConfig::new().mount("/site"));
    let two = files(dir.path(), StaticFilesConfig::new().mount("/site/admin"));
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("puerto efímero");
    let err = Arc::new(AxumPlatform::new().static_files(one).static_files(two))
        .serve(
            App::new().without_health().build().expect("app").http,
            listener,
            ShutdownSignal::new(),
        )
        .err()
        .expect("los montajes solapados deben impedir arrancar");
    assert!(
        err.to_string().contains("solapa"),
        "mensaje inesperado: {err}"
    );
}
