//! G-01 a G-08 (§20, §10.9): el motor GraphQL y el montaje `/graphql`.
//!
//! G-01 a G-03 corren además en proceso contra `SchemaBuilder` y
//! `Schema::execute`; el resto levanta un `AxumPlatform` real en
//! `127.0.0.1:0` con la feature `graphql` y apaga con `ShutdownSignal`.

#![cfg(feature = "graphql")]

use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper_util::rt::TokioIo;
use rdd::AxumPlatform;
use rdd::graphql::{
    GraphqlContext, GraphqlHttp, GraphqlHttpConfig, GraphqlObject, GraphqlRequest, GraphqlRoot,
    GraphqlStream, RootField, RootKind, Schema, TypeRef, TypeRegistry, graphql_object, resolved,
};
use rdd::http::{
    HeaderMap, Middleware, NextFn, Platform, Request, RequestExt, ShutdownSignal, StatusCode,
    fn_middleware, header,
};
use rdd::{App, Error};
use serde_json::{Value, json};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::timeout;
use uuid::Uuid;

const WAIT: Duration = Duration::from_secs(10);
const BODY_LIMIT: usize = 512;

// --- esquema de prueba ---------------------------------------------------

/// Escalares de §10.9 sobre un objeto con nombre.
#[derive(GraphqlObject)]
struct Numbers {
    int: i32,
    float: f64,
    id: Uuid,
    text: String,
    flag: bool,
}

#[derive(GraphqlObject)]
struct Nested {
    value: String,
}

#[derive(GraphqlObject)]
struct Level3 {
    leaf: String,
}

#[derive(GraphqlObject)]
struct Level2 {
    three: Level3,
}

#[derive(GraphqlObject)]
struct Level1 {
    two: Level2,
}

/// Identidad mínima: lo que un `JwtMiddleware` deja en las extensiones.
#[derive(Clone)]
struct Identity {
    email: String,
}

/// Servicio del contenedor, para comprobar `ctx.resolve` (RF-239).
#[derive(Clone)]
struct Greeting(String);

impl rdd::http::FromContainer for Greeting {
    const NAME: &'static str = "Greeting";

    fn from_container(container: &rdd::di::Container) -> rdd::Result<Self> {
        container
            .get::<Self>()
            .ok_or_else(|| Error::not_found("Greeting"))
    }
}

/// Productor de la suscripción; la prueba controla el emisor.
#[derive(Clone, Default)]
struct TickFeed {
    sender: Arc<Mutex<Option<tokio::sync::mpsc::Sender<i32>>>>,
}

impl TickFeed {
    fn receiver(&self) -> tokio::sync::mpsc::Receiver<i32> {
        let (tx, rx) = tokio::sync::mpsc::channel(8);
        *self.sender.lock().expect("feed envenenado") = Some(tx);
        rx
    }

    fn drop_sender(&self) {
        let _ = self.sender.lock().expect("feed envenenado").take();
    }

    async fn send(&self, value: i32) -> bool {
        let sender = self.sender.lock().expect("feed envenenado").clone();
        match sender {
            Some(tx) => tx.send(value).await.is_ok(),
            None => false,
        }
    }
}

#[derive(Default)]
struct Query;

#[graphql_object(query)]
impl Query {
    /// Saludo fijo.
    async fn greeting(&self, _ctx: &GraphqlContext) -> rdd::Result<String> {
        Ok("hola".to_owned())
    }

    async fn hidden(&self, _ctx: &GraphqlContext) -> rdd::Result<String> {
        Ok("oculto".to_owned())
    }

    async fn numbers(&self, _ctx: &GraphqlContext) -> rdd::Result<Numbers> {
        Ok(Numbers {
            int: 7,
            float: 1.5,
            id: Uuid::nil(),
            text: "texto".to_owned(),
            flag: true,
        })
    }

    async fn nested(&self, _ctx: &GraphqlContext) -> rdd::Result<Nested> {
        Ok(Nested {
            value: "fondo".to_owned(),
        })
    }

    async fn level1(&self, _ctx: &GraphqlContext) -> rdd::Result<Level1> {
        Ok(Level1 {
            two: Level2 {
                three: Level3 {
                    leaf: "fondo".to_owned(),
                },
            },
        })
    }

    async fn echo(
        &self,
        _ctx: &GraphqlContext,
        text: String,
        times: Option<i32>,
    ) -> rdd::Result<String> {
        Ok(text.repeat(times.unwrap_or(1) as usize))
    }

    async fn optional(&self, _ctx: &GraphqlContext) -> rdd::Result<Option<String>> {
        Ok(None)
    }

    /// Falla siendo nullable: el resto de `data` se conserva (RF-236).
    async fn boom(&self, _ctx: &GraphqlContext) -> rdd::Result<Option<String>> {
        Err(Error::not_found("no hay nada"))
    }

    /// Falla siendo no-null: `data` entero es `null` (RF-236).
    async fn fail_hard(&self, _ctx: &GraphqlContext) -> rdd::Result<String> {
        Err(Error::invalid("regla incumplida"))
    }

    async fn panic(&self, _ctx: &GraphqlContext) -> rdd::Result<String> {
        panic!("explosión de prueba")
    }

    async fn whoami(&self, ctx: &GraphqlContext) -> rdd::Result<String> {
        ctx.ext::<Identity>()
            .map(|identity| identity.email)
            .ok_or(Error::Unauthorized)
    }

    async fn service_greeting(&self, ctx: &GraphqlContext) -> rdd::Result<String> {
        Ok(ctx.resolve::<Greeting>()?.0)
    }
}

#[derive(Default)]
struct Mutation;

#[graphql_object(mutation)]
impl Mutation {
    async fn add(&self, _ctx: &GraphqlContext, a: i32, b: i32) -> rdd::Result<i32> {
        Ok(a + b)
    }
}

#[derive(Default)]
struct Subscription;

#[graphql_object(subscription)]
impl Subscription {
    /// Un entero por envío del productor de la prueba.
    async fn counter(&self, ctx: &GraphqlContext) -> rdd::Result<GraphqlStream<i32>> {
        let feed = ctx
            .get::<TickFeed>()
            .ok_or_else(|| Error::not_found("falta TickFeed"))?;
        Ok(GraphqlStream::new(feed.receiver()))
    }
}

fn schema() -> Schema {
    Schema::builder()
        .query(Query)
        .mutation(Mutation)
        .subscription(Subscription)
        .build()
        .expect("el esquema de prueba debe construirse")
}

fn context() -> GraphqlContext {
    GraphqlContext::empty()
}

async fn run(query: &str) -> Value {
    schema()
        .execute(&context(), GraphqlRequest::new(query))
        .await
}

async fn run_with_variables(query: &str, variables: Value) -> Value {
    schema()
        .execute(
            &context(),
            GraphqlRequest {
                query: query.to_owned(),
                operation_name: None,
                variables: Some(variables),
            },
        )
        .await
}

// --- app y servidor ------------------------------------------------------

fn seed_identity() -> impl Middleware {
    fn_middleware(|mut req: Request, next: NextFn| async move {
        let bearer = req
            .headers()
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok());
        if bearer == Some("Bearer ana") {
            req.extensions_mut().insert(Identity {
                email: "ana@correo.com".to_owned(),
            });
        }
        next.run(req).await
    })
}

fn require_identity(req: &Request) -> rdd::Result<()> {
    req.ext::<Identity>().map(|_| ()).ok_or(Error::Unauthorized)
}

fn http_app(feed: TickFeed) -> rdd::http::HttpApp {
    App::new()
        .without_health()
        .body_limit(BODY_LIMIT)
        .middleware(seed_identity())
        .provide(Greeting("hola".to_owned()))
        .provide(feed)
        .build()
        .expect("la app de prueba debe construirse")
        .http
}

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

async fn start(config: GraphqlHttpConfig, feed: TickFeed) -> Running {
    let graphql = GraphqlHttp::new(schema()).config(config);
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("no se pudo vincular el puerto efímero");
    let addr = listener.local_addr().expect("socket sin dirección");
    let shutdown = ShutdownSignal::new();
    let serving = Arc::new(AxumPlatform::new().graphql(graphql))
        .serve(http_app(feed), listener, shutdown.clone())
        .unwrap_or_else(|err| panic!("el montaje /graphql no arrancó: {err}"));
    let task = tokio::spawn(serving);
    Running {
        addr,
        shutdown,
        task,
    }
}

// --- cliente HTTP --------------------------------------------------------

struct Reply {
    status: StatusCode,
    headers: HeaderMap,
    body: Bytes,
}

async fn connect(
    addr: std::net::SocketAddr,
) -> hyper::client::conn::http1::SendRequest<Full<Bytes>> {
    let stream = TcpStream::connect(addr)
        .await
        .unwrap_or_else(|err| panic!("no se pudo conectar a {addr}: {err}"));
    let (sender, conn) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
        .await
        .expect("falló el handshake HTTP/1.1");
    tokio::spawn(async move {
        let _ = conn.await;
    });
    sender
}

async fn send(addr: std::net::SocketAddr, req: http::Request<Full<Bytes>>) -> Reply {
    let mut sender = connect(addr).await;
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

async fn send_stream(
    addr: std::net::SocketAddr,
    req: http::Request<Full<Bytes>>,
) -> (StatusCode, HeaderMap, Incoming) {
    let mut sender = connect(addr).await;
    let resp = sender
        .send_request(req)
        .await
        .unwrap_or_else(|err| panic!("la petición SSE no obtuvo respuesta: {err}"));
    let (parts, body) = resp.into_parts();
    (parts.status, parts.headers, body)
}

fn gql(value: &Value) -> http::Request<Full<Bytes>> {
    gql_with(value, "application/json", &[])
}

fn gql_with(
    value: &Value,
    accept: &str,
    extra: &[(&'static str, &'static str)],
) -> http::Request<Full<Bytes>> {
    let mut builder = http::Request::builder()
        .method("POST")
        .uri("/graphql")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ACCEPT, accept);
    for (name, value) in extra {
        builder = builder.header(*name, *value);
    }
    builder
        .body(Full::new(Bytes::from(value.to_string())))
        .unwrap()
}

fn body_json(reply: &Reply) -> Value {
    serde_json::from_slice(&reply.body).unwrap_or_else(|_| {
        panic!(
            "cuerpo no JSON ({}): {:?}",
            reply.body.len(),
            String::from_utf8_lossy(&reply.body)
        )
    })
}

fn cache_control(reply: &Reply) -> Option<&str> {
    reply
        .headers
        .get(header::CACHE_CONTROL)
        .and_then(|value| value.to_str().ok())
}

fn header_str(headers: &HeaderMap, name: header::HeaderName) -> Option<&str> {
    headers.get(name).and_then(|value| value.to_str().ok())
}

/// URL-encode de un valor para la query de `GET /graphql`.
fn encode(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

// --- G-01 ----------------------------------------------------------------

#[derive(Default)]
struct DuplicateQuery;

#[graphql_object(query)]
impl DuplicateQuery {
    async fn same(&self, _ctx: &GraphqlContext) -> rdd::Result<String> {
        Ok("una".to_owned())
    }
}

#[derive(Default)]
struct DuplicateMutation;

#[graphql_object(mutation)]
impl DuplicateMutation {
    async fn same(&self, _ctx: &GraphqlContext) -> rdd::Result<String> {
        Ok("otra".to_owned())
    }
}

/// Raíz manual con un nombre reservado: la macro lo rechaza, pero `build` también.
#[derive(Default)]
struct Reserved;

impl GraphqlRoot for Reserved {
    fn root_fields() -> Vec<RootField> {
        vec![RootField {
            name: "__bad",
            description: None,
            args: Vec::new(),
            ty: TypeRef::required("String"),
            kind: RootKind::Query,
            resolve: Some(Arc::new(|_ctx, _args| {
                Box::pin(async { Ok(resolved(String::new())) })
            })),
            subscribe: None,
        }]
    }

    fn collect_root_types(_types: &mut TypeRegistry) {}
}

#[test]
fn g01_build_valida_raices_y_nombres() {
    let built = Schema::builder()
        .query(Query)
        .mutation(Mutation)
        .subscription(Subscription)
        .build();
    assert!(built.is_ok(), "las tres raíces deben construir");

    let Err(duplicated) = Schema::builder()
        .query(DuplicateQuery)
        .mutation(DuplicateMutation)
        .build()
    else {
        panic!("dos raíces con el mismo campo deben fallar");
    };
    let message = duplicated.to_string();
    assert!(message.contains("`same`"), "{message}");
    assert!(
        message.contains("Query") && message.contains("Mutation"),
        "{message}"
    );

    let Err(reserved) = Schema::builder().query(Reserved).build() else {
        panic!("`__` está reservado");
    };
    let message = reserved.to_string();
    assert!(message.contains("`__bad`"), "{message}");
    assert!(message.contains("reservado"), "{message}");
}

// --- G-02 ----------------------------------------------------------------

#[tokio::test]
async fn g02_variables_alias_fragmentos_y_directivas() {
    let document = r#"
        query Demo($show: Boolean!) {
          x: greeting
          numbers { ...Numbers }
          hidden @skip(if: $show)
          extra: hidden @include(if: $show)
          echoed: echo(text: "ab", times: 3)
        }
        fragment Numbers on Numbers { int float id text flag }
    "#;
    let included = run_with_variables(document, json!({ "show": true })).await;

    assert!(included.get("errors").is_none(), "sin errores: {included}");
    let data = &included["data"];
    assert_eq!(data["x"], "hola");
    assert_eq!(data["extra"], "oculto");
    assert_eq!(data["echoed"], "ababab");
    assert!(data.get("hidden").is_none(), "skip elimina la clave");

    let skipped = run_with_variables(document, json!({ "show": false })).await;
    assert_eq!(
        skipped["data"]["hidden"], "oculto",
        "skip(if: false) incluye"
    );
    assert!(
        skipped["data"].get("extra").is_none(),
        "include(if: false) elimina la clave"
    );

    let numbers = &data["numbers"];
    assert_eq!(numbers["int"], 7);
    assert_eq!(numbers["float"], 1.5);
    assert_eq!(numbers["id"], Uuid::nil().to_string());
    assert_eq!(numbers["text"], "texto");
    assert_eq!(numbers["flag"], true);

    let mutation = run_with_variables(
        "mutation($a: Int!, $b: Int!) { add(a: $a, b: $b) }",
        json!({ "a": 2, "b": 3 }),
    )
    .await;
    assert_eq!(mutation["data"]["add"], 5);
}

#[tokio::test]
async fn g02_valor_nulo_y_seleccion_minima() {
    let data = run("{ optional nested { value } }").await;
    assert_eq!(data["data"]["optional"], Value::Null);
    assert_eq!(data["data"]["nested"]["value"], "fondo");
}

// --- G-03 ----------------------------------------------------------------

#[tokio::test]
async fn g03_errores_de_validacion_y_burbujeo() {
    let unknown = run("{ missing }").await;
    assert_eq!(unknown["data"], Value::Null);
    assert!(
        unknown["errors"][0]["message"]
            .as_str()
            .is_some_and(|message| message.contains("desconocido")),
        "{unknown}"
    );
    assert!(
        unknown["errors"][0]["locations"][0]["line"].is_number(),
        "{unknown}"
    );

    let missing_arg = run("{ echo }").await;
    assert!(
        missing_arg["errors"]
            .as_array()
            .is_some_and(|errors| errors.iter().any(|error| error["message"]
                .as_str()
                .is_some_and(|message| message.contains("obligatorio")))),
        "{missing_arg}"
    );

    let bad_variable =
        run_with_variables("query($t: Int!) { echo(text: $t) }", json!({ "t": 1 })).await;
    assert!(
        bad_variable["errors"]
            .as_array()
            .is_some_and(|errors| !errors.is_empty()),
        "{bad_variable}"
    );

    // Nullable que falla: data conserva el resto (RF-236).
    let nullable = run("{ greeting boom }").await;
    assert_eq!(nullable["data"]["greeting"], "hola");
    assert_eq!(nullable["data"]["boom"], Value::Null);
    assert_eq!(nullable["errors"][0]["path"][0], "boom");

    // No-null que falla: `data` entero es null.
    let required = run("{ greeting failHard }").await;
    assert_eq!(required["data"], Value::Null);
    assert_eq!(required["errors"][0]["path"][0], "failHard");
}

#[tokio::test]
async fn g03_profundidad_maxima_por_http() {
    let running = start(GraphqlHttpConfig::new().max_depth(2), TickFeed::default()).await;
    let addr = running.addr;

    let ok = send(addr, gql(&json!({ "query": "{ numbers { text } }" }))).await;
    assert_eq!(ok.status, StatusCode::OK);
    assert_eq!(body_json(&ok)["data"]["numbers"]["text"], "texto");

    let too_deep = send(
        addr,
        gql(&json!({ "query": "{ level1 { two { three { leaf } } } }" })),
    )
    .await;
    assert_eq!(too_deep.status, StatusCode::OK);
    assert!(
        body_json(&too_deep)["errors"][0]["message"]
            .as_str()
            .is_some_and(|message| message.contains("profundidad")),
        "{:?}",
        body_json(&too_deep)
    );

    running.stop().await;
}

// --- G-04 ----------------------------------------------------------------

#[tokio::test]
async fn g04_introspeccion() {
    let schema = run(r#"{
            __schema {
                queryType { name }
                mutationType { name }
                subscriptionType { name }
                types { kind name fields { name } }
            }
        }"#)
    .await;
    assert!(schema.get("errors").is_none(), "{schema}");
    let schema = &schema["data"]["__schema"];
    assert_eq!(schema["queryType"]["name"], "Query");
    assert_eq!(schema["mutationType"]["name"], "Mutation");
    assert_eq!(schema["subscriptionType"]["name"], "Subscription");
    let types = schema["types"].as_array().expect("lista de tipos");
    assert!(
        types
            .iter()
            .any(|item| item["name"] == "Numbers" && item["kind"] == "OBJECT"),
        "{types:?}"
    );
    assert!(
        types
            .iter()
            .any(|item| item["name"] == "ID" && item["kind"] == "SCALAR"),
        "{types:?}"
    );
    let numbers = types
        .iter()
        .find(|item| item["name"] == "Numbers")
        .expect("falta Numbers");
    let fields: Vec<&str> = numbers["fields"]
        .as_array()
        .expect("campos")
        .iter()
        .map(|field| field["name"].as_str().unwrap())
        .collect();
    assert!(
        fields.contains(&"int") && fields.contains(&"flag"),
        "{fields:?}"
    );

    let one = run(r#"{ __type(name: "Numbers") { name kind } }"#).await;
    assert_eq!(one["data"]["__type"]["name"], "Numbers");
    assert_eq!(one["data"]["__type"]["kind"], "OBJECT");

    let unknown = run(r#"{ __type(name: "Inexistente") { name } }"#).await;
    assert_eq!(unknown["data"]["__type"], Value::Null);

    let typename = run("{ __typename numbers { __typename text } }").await;
    assert_eq!(typename["data"]["__typename"], "Query");
    assert_eq!(typename["data"]["numbers"]["__typename"], "Numbers");
}

// --- G-05 ----------------------------------------------------------------

#[tokio::test]
async fn g05_transporte_y_no_store() {
    let running = start(GraphqlHttpConfig::new(), TickFeed::default()).await;
    let addr = running.addr;

    let wrong_type = http::Request::builder()
        .method("POST")
        .uri("/graphql")
        .header(header::CONTENT_TYPE, "text/plain")
        .body(Full::new(Bytes::from_static(b"{}")))
        .unwrap();
    let reply = send(addr, wrong_type).await;
    assert_eq!(reply.status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert_eq!(cache_control(&reply), Some("no-store"));

    let too_large = http::Request::builder()
        .method("POST")
        .uri("/graphql")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Full::new(Bytes::from(vec![b' '; BODY_LIMIT * 4])))
        .unwrap();
    let reply = send(addr, too_large).await;
    assert_eq!(reply.status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(cache_control(&reply), Some("no-store"));

    let broken = http::Request::builder()
        .method("POST")
        .uri("/graphql")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Full::new(Bytes::from_static(b"{no es json")))
        .unwrap();
    let reply = send(addr, broken).await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST);
    assert_eq!(cache_control(&reply), Some("no-store"));

    let not_object = send(addr, gql(&json!([{ "query": "{ greeting }" }]))).await;
    assert_eq!(not_object.status, StatusCode::BAD_REQUEST);

    let without_query = send(addr, gql(&json!({ "variables": {} }))).await;
    assert_eq!(without_query.status, StatusCode::BAD_REQUEST);

    let put = http::Request::builder()
        .method("PUT")
        .uri("/graphql")
        .body(Full::new(Bytes::new()))
        .unwrap();
    let reply = send(addr, put).await;
    assert_eq!(reply.status, StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(header_str(&reply.headers, header::ALLOW), Some("GET, POST"));
    assert_eq!(cache_control(&reply), Some("no-store"));

    // Un error de documento viaja con 200 y no-store (RF-236).
    let document_error = send(addr, gql(&json!({ "query": "{ missing }" }))).await;
    assert_eq!(document_error.status, StatusCode::OK);
    assert_eq!(cache_control(&document_error), Some("no-store"));
    assert!(body_json(&document_error)["errors"].is_array());

    running.stop().await;
}

// --- G-06 ----------------------------------------------------------------

#[tokio::test]
async fn g06_get_explorer_y_metodos() {
    let running = start(GraphqlHttpConfig::new(), TickFeed::default()).await;
    let addr = running.addr;

    let document = "query($text: String!) { echo(text: $text) }";
    let variables = r#"{"text":"hola mundo"}"#;
    let uri = format!(
        "/graphql?query={}&variables={}",
        encode(document),
        encode(variables)
    );
    let get = http::Request::builder()
        .method("GET")
        .uri(uri)
        .body(Full::new(Bytes::new()))
        .unwrap();
    let reply = send(addr, get).await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(body_json(&reply)["data"]["echo"], "hola mundo");
    assert_eq!(cache_control(&reply), Some("no-store"));

    for document in ["mutation { add(a: 1, b: 2) }", "subscription { counter }"] {
        let uri = format!("/graphql?query={}", encode(document));
        let get = http::Request::builder()
            .method("GET")
            .uri(uri)
            .body(Full::new(Bytes::new()))
            .unwrap();
        let reply = send(addr, get).await;
        assert_eq!(reply.status, StatusCode::METHOD_NOT_ALLOWED, "{document}");
        assert_eq!(header_str(&reply.headers, header::ALLOW), Some("POST"));
    }

    // Explorer: HTML autocontenido con CSP estricta (RF-241).
    let get = http::Request::builder()
        .method("GET")
        .uri("/graphql")
        .body(Full::new(Bytes::new()))
        .unwrap();
    let reply = send(addr, get).await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(
        header_str(&reply.headers, header::CONTENT_TYPE),
        Some("text/html; charset=utf-8")
    );
    let csp = header_str(&reply.headers, header::CONTENT_SECURITY_POLICY).unwrap_or_default();
    assert_eq!(
        csp,
        "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; \
         connect-src 'self'"
    );
    assert_eq!(
        header_str(&reply.headers, header::X_CONTENT_TYPE_OPTIONS),
        Some("nosniff")
    );
    let html = String::from_utf8(reply.body.to_vec()).expect("HTML UTF-8");
    assert!(html.to_lowercase().contains("<html"), "{html}");
    assert!(html.contains("text/event-stream"), "el explorer lee SSE");
    assert!(
        !html.contains("src=\"http") && !html.contains("href=\"http"),
        "{html}"
    );

    running.stop().await;

    let disabled = start(
        GraphqlHttpConfig::new().explorer(false),
        TickFeed::default(),
    )
    .await;
    let get = http::Request::builder()
        .method("GET")
        .uri("/graphql")
        .body(Full::new(Bytes::new()))
        .unwrap();
    let reply = send(disabled.addr, get).await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND);
    assert_eq!(cache_control(&reply), Some("no-store"));
    disabled.stop().await;
}

// --- G-07 ----------------------------------------------------------------

/// Siguiente bloque SSE (separado por una línea en blanco).
async fn next_block(body: &mut Incoming, pending: &mut Vec<u8>) -> String {
    loop {
        if let Some(position) = pending.windows(2).position(|pair| pair == b"\n\n") {
            let block: Vec<u8> = pending.drain(..position + 2).collect();
            return String::from_utf8(block).expect("SSE no es UTF-8");
        }
        let frame = timeout(WAIT, body.frame())
            .await
            .expect("el flujo SSE no emitió nada")
            .expect("el flujo SSE terminó")
            .expect("marco inválido");
        if let Ok(data) = frame.into_data() {
            pending.extend_from_slice(&data);
        }
    }
}

fn data_of(block: &str) -> Option<Value> {
    let data = block.lines().find_map(|line| line.strip_prefix("data: "))?;
    Some(serde_json::from_str(data).expect("data no es JSON"))
}

#[tokio::test]
async fn g07_suscripcion_sse_y_cancelacion() {
    let feed = TickFeed::default();
    let running = start(GraphqlHttpConfig::new(), feed.clone()).await;
    let addr = running.addr;

    let no_accept = send(addr, gql(&json!({ "query": "subscription { counter }" }))).await;
    assert_eq!(no_accept.status, StatusCode::NOT_ACCEPTABLE);
    assert_eq!(cache_control(&no_accept), Some("no-store"));

    let (status, headers, mut body) = send_stream(
        addr,
        gql_with(
            &json!({ "query": "subscription { counter }" }),
            "text/event-stream",
            &[],
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        header_str(&headers, header::CONTENT_TYPE),
        Some("text/event-stream")
    );
    assert_eq!(
        header_str(&headers, header::CACHE_CONTROL),
        Some("no-store")
    );

    let mut pending = Vec::new();
    let first = next_block(&mut body, &mut pending).await;
    assert_eq!(first, ": ping\n\n", "el flujo abre con un latido");

    assert!(feed.send(7).await);
    let event = next_block(&mut body, &mut pending).await;
    assert!(event.starts_with("event: next\n"), "{event:?}");
    assert_eq!(data_of(&event).expect("data")["data"]["counter"], 7);

    // Al desconectar el cliente la cola se cierra y el productor lo nota.
    drop(body);
    let closed = timeout(WAIT, async {
        loop {
            if !feed.send(8).await {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await;
    assert!(
        closed.is_ok(),
        "la suscripción siguió viva tras desconectar"
    );

    // Cerrar el productor emite `event: complete`.
    let (status, _, mut body) = send_stream(
        addr,
        gql_with(
            &json!({ "query": "subscription { counter }" }),
            "text/event-stream",
            &[],
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let mut pending = Vec::new();
    assert_eq!(next_block(&mut body, &mut pending).await, ": ping\n\n");
    feed.drop_sender();
    let complete = next_block(&mut body, &mut pending).await;
    assert_eq!(complete, "event: complete\ndata: {}\n\n");

    running.stop().await;
}

// --- G-08 ----------------------------------------------------------------

/// Captura de `tracing` para comprobar que el detalle del `panic` va al log.
static LOGS: Mutex<Vec<u8>> = Mutex::new(Vec::new());
static LOG_INIT: OnceLock<()> = OnceLock::new();

struct Capture;

impl std::io::Write for Capture {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        LOGS.lock().expect("log envenenado").extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn init_logs() {
    LOG_INIT.get_or_init(|| {
        let subscriber = tracing_subscriber::fmt()
            .with_writer(|| Capture)
            .with_ansi(false)
            .with_max_level(tracing::Level::ERROR)
            .finish();
        let _ = tracing::subscriber::set_global_default(subscriber);
    });
}

#[tokio::test]
async fn g08_entrada_identidad_y_panico() {
    init_logs();
    let running = start(
        GraphqlHttpConfig::new().authorize(require_identity),
        TickFeed::default(),
    )
    .await;
    let addr = running.addr;

    // Sin token: explorer, POST y suscripción responden 401 (RF-239).
    let explorer = http::Request::builder()
        .method("GET")
        .uri("/graphql")
        .body(Full::new(Bytes::new()))
        .unwrap();
    assert_eq!(send(addr, explorer).await.status, StatusCode::UNAUTHORIZED);

    let post = send(addr, gql(&json!({ "query": "{ greeting }" }))).await;
    assert_eq!(post.status, StatusCode::UNAUTHORIZED);
    assert_eq!(body_json(&post)["status"], 401);

    let subscription = send(addr, gql(&json!({ "query": "subscription { counter }" }))).await;
    assert_eq!(subscription.status, StatusCode::UNAUTHORIZED);

    // Con token: `ctx.ext` y `ctx.resolve` ven identidad y provider.
    let seen = send(
        addr,
        gql_with(
            &json!({ "query": "{ whoami serviceGreeting }" }),
            "application/json",
            &[("authorization", "Bearer ana")],
        ),
    )
    .await;
    assert_eq!(seen.status, StatusCode::OK);
    let body = body_json(&seen);
    assert_eq!(body["data"]["whoami"], "ana@correo.com");
    assert_eq!(body["data"]["serviceGreeting"], "hola");

    // Un `panic` de resolutor responde "error interno" y no tumba el servidor.
    let crashed = send(
        addr,
        gql_with(
            &json!({ "query": "{ panic }" }),
            "application/json",
            &[("authorization", "Bearer ana")],
        ),
    )
    .await;
    assert_eq!(crashed.status, StatusCode::OK);
    let body = body_json(&crashed);
    assert_eq!(body["data"], Value::Null);
    assert_eq!(body["errors"][0]["message"], "error interno");
    assert_eq!(body["errors"][0]["path"][0], "panic");
    assert!(
        !body.to_string().contains("explosión"),
        "el detalle del panic no viaja al cliente"
    );

    let alive = send(
        addr,
        gql_with(
            &json!({ "query": "{ greeting }" }),
            "application/json",
            &[("authorization", "Bearer ana")],
        ),
    )
    .await;
    assert_eq!(body_json(&alive)["data"]["greeting"], "hola");

    // El detalle queda en `tracing` (RN-040).
    let logged = timeout(WAIT, async {
        loop {
            if String::from_utf8_lossy(&LOGS.lock().expect("log envenenado"))
                .contains("el resolutor GraphQL falló")
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await;
    assert!(logged.is_ok(), "el panic no dejó rastro en tracing");

    running.stop().await;
}
