//! Semántica HTTP de `/graphql`, neutral respecto al servidor (§10.9, RP-030).
//!
//! Aquí no hay sockets: se recibe un [`Request`] ya preparado por la cadena
//! global (RP-028), se decide el transporte —JSON, explorer o SSE— y se devuelve
//! un [`GraphqlHttpResponse`]. El montaje y el bombeo del cuerpo son de
//! `rdd-axum` (RP-031).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use serde_json::Value;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use rdd_core::error::Problem;
use rdd_core::http::{
    HeaderMap, HeaderName, HeaderValue, Request, Response, StatusCode, Uri,
    header::{ACCEPT, CACHE_CONTROL, CONTENT_TYPE},
    json_response, normalize_path, problem_response,
    response::error_response_for,
};

use crate::parser::OperationKind;
use crate::schema::{
    GraphqlContext, GraphqlError, GraphqlRequest, GraphqlStream, Schema, error_value,
    select_operation,
};
use crate::validate::Prepared;

/// Latido de una suscripción SSE (`RF-238`).
const PING: Duration = Duration::from_secs(15);

/// Guardián ejecutado después de los globales (`RF-239`).
type AuthorizeFn = Arc<dyn Fn(&Request) -> rdd_core::Result<()> + Send + Sync>;

/// Configuración del montaje `/graphql` (`RF-242`).
pub struct GraphqlHttpConfig {
    path: String,
    explorer: bool,
    max_depth: usize,
    authorize: Option<AuthorizeFn>,
}

impl Default for GraphqlHttpConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl GraphqlHttpConfig {
    /// `/graphql`, explorer activo y profundidad máxima 16.
    pub fn new() -> Self {
        Self {
            path: "/graphql".to_owned(),
            explorer: true,
            max_depth: crate::DEFAULT_MAX_DEPTH,
            authorize: None,
        }
    }

    /// Ruta donde se monta (`RF-233`).
    pub fn path(mut self, path: impl Into<String>) -> Self {
        self.path = path.into();
        self
    }

    /// GET sin `query` sirve el explorer; `false` responde 404 (`RF-241`).
    pub fn explorer(mut self, enabled: bool) -> Self {
        self.explorer = enabled;
        self
    }

    /// Anidamiento máximo de conjuntos de selección (`RF-235`, `RF-242`).
    pub fn max_depth(mut self, max: usize) -> Self {
        self.max_depth = max;
        self
    }

    /// Se ejecuta después de los globales; `Err` se responde como Problem Details.
    pub fn authorize<F>(mut self, f: F) -> Self
    where
        F: Fn(&Request) -> rdd_core::Result<()> + Send + Sync + 'static,
    {
        self.authorize = Some(Arc::new(f));
        self
    }

    /// Ruta configurada, para que la plataforma valide el montaje.
    pub fn mount_path(&self) -> &str {
        &self.path
    }

    /// Si el explorer está habilitado.
    pub fn explorer_enabled(&self) -> bool {
        self.explorer
    }

    /// Profundidad máxima configurada.
    pub fn max_depth_limit(&self) -> usize {
        self.max_depth
    }

    /// Valida los valores que solo se conocen al arrancar (`RF-242`).
    pub fn validate(&self) -> rdd_core::Result<()> {
        if self.path.is_empty() || !self.path.starts_with('/') {
            return Err(rdd_core::Error::internal(anyhow::anyhow!(
                "`GraphqlHttpConfig::path` debe empezar por `/` y no estar vacío: {:?}",
                self.path
            )));
        }
        if self.max_depth == 0 {
            return Err(rdd_core::Error::internal(anyhow::anyhow!(
                "`GraphqlHttpConfig::max_depth` debe ser mayor que 0"
            )));
        }
        Ok(())
    }
}

/// Punto de entrada GraphQL sobre el mismo `HttpApp` (`RF-233`).
pub struct GraphqlHttp {
    schema: Arc<Schema>,
    config: GraphqlHttpConfig,
    shutdown: CancellationToken,
}

impl GraphqlHttp {
    /// Entrada con la configuración por defecto.
    pub fn new(schema: Schema) -> Self {
        Self {
            schema: Arc::new(schema),
            config: GraphqlHttpConfig::new(),
            shutdown: CancellationToken::new(),
        }
    }

    /// Sustituye la configuración por defecto.
    pub fn config(mut self, config: GraphqlHttpConfig) -> Self {
        self.config = config;
        self
    }

    /// Ruta donde la plataforma debe montarlo (`RF-233`).
    pub fn path(&self) -> &str {
        self.config.mount_path()
    }

    /// Configuración efectiva, para que la plataforma valide el montaje.
    pub fn settings(&self) -> &GraphqlHttpConfig {
        &self.config
    }

    /// `req` ya pasó por `HttpApp::prepare` (`RP-028`).
    pub async fn handle(&self, req: Request) -> GraphqlHttpResponse {
        let instance = normalize_path(req.uri().path());

        if let Some(authorize) = &self.config.authorize
            && let Err(err) = authorize(&req)
        {
            return GraphqlHttpResponse::Json(no_store(error_response_for(&err, &instance)));
        }

        match req.method().as_str() {
            "GET" => self.get(&req, &instance).await,
            "POST" => self.post(req, &instance).await,
            _ => GraphqlHttpResponse::Json(no_store(problem(
                StatusCode::METHOD_NOT_ALLOWED,
                "Method Not Allowed",
                "`/graphql` solo admite GET y POST",
                &instance,
                Some("GET, POST"),
            ))),
        }
    }

    /// Cancela las suscripciones en vuelo al apagar (`RF-238`).
    pub async fn shutdown(&self) {
        self.shutdown.cancel();
    }

    async fn get(&self, req: &Request, instance: &str) -> GraphqlHttpResponse {
        let params = match query_params(req.uri()) {
            Ok(params) => params,
            Err(()) => {
                return GraphqlHttpResponse::Json(no_store(bad_request(
                    "la query de la URL no está bien codificada",
                    instance,
                )));
            }
        };

        let Some(query) = params.get("query") else {
            if self.config.explorer {
                return GraphqlHttpResponse::Html(no_store(explorer_response()));
            }
            return GraphqlHttpResponse::Json(no_store(problem(
                StatusCode::NOT_FOUND,
                "Not Found",
                "el explorer de GraphQL está desactivado",
                instance,
                None,
            )));
        };

        let variables = match params.get("variables") {
            None => None,
            Some(text) => match serde_json::from_str::<Value>(text) {
                Ok(Value::Null) => None,
                Ok(value @ Value::Object(_)) => Some(value),
                Ok(_) => {
                    return GraphqlHttpResponse::Json(no_store(bad_request(
                        "`variables` debe ser un objeto JSON",
                        instance,
                    )));
                }
                Err(_) => {
                    return GraphqlHttpResponse::Json(no_store(bad_request(
                        "`variables` no es JSON válido",
                        instance,
                    )));
                }
            },
        };

        let request = GraphqlRequest {
            query: query.clone(),
            operation_name: params.get("operationName").cloned(),
            variables,
        };

        let kind = match select_kind(&request) {
            Ok(kind) => kind,
            Err(errors) => {
                return GraphqlHttpResponse::Json(no_store(json_response(
                    StatusCode::OK,
                    &error_value(errors),
                )));
            }
        };
        if kind != OperationKind::Query {
            return GraphqlHttpResponse::Json(no_store(problem(
                StatusCode::METHOD_NOT_ALLOWED,
                "Method Not Allowed",
                "una mutación o suscripción no se admite por GET",
                instance,
                Some("POST"),
            )));
        }

        let prepared = match crate::validate::analyze(&self.schema, &request, self.config.max_depth)
        {
            Ok(prepared) => prepared,
            Err(errors) => {
                return GraphqlHttpResponse::Json(no_store(json_response(
                    StatusCode::OK,
                    &error_value(errors),
                )));
            }
        };
        log_operation(&prepared);

        let ctx = GraphqlContext::from_request(req);
        GraphqlHttpResponse::Json(no_store(json_response(
            StatusCode::OK,
            &crate::execute::execute(&self.schema, &ctx, prepared).await,
        )))
    }

    async fn post(&self, req: Request, instance: &str) -> GraphqlHttpResponse {
        if !is_json_content(req.headers()) {
            return GraphqlHttpResponse::Json(no_store(problem(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "Unsupported Media Type",
                "el cuerpo debe declarar `Content-Type: application/json`",
                instance,
                None,
            )));
        }

        let value: Value = match serde_json::from_slice(req.body()) {
            Ok(value) => value,
            Err(_) => {
                return GraphqlHttpResponse::Json(no_store(bad_request(
                    "el cuerpo no es JSON válido",
                    instance,
                )));
            }
        };
        let Some(object) = value.as_object() else {
            return GraphqlHttpResponse::Json(no_store(bad_request(
                "el cuerpo debe ser un objeto `{query, operationName?, variables?}`",
                instance,
            )));
        };
        let Some(query) = object.get("query").and_then(Value::as_str) else {
            return GraphqlHttpResponse::Json(no_store(bad_request(
                "falta el campo `query` con el documento",
                instance,
            )));
        };
        let operation_name = match object.get("operationName") {
            None | Some(Value::Null) => None,
            Some(Value::String(name)) => Some(name.clone()),
            Some(_) => {
                return GraphqlHttpResponse::Json(no_store(bad_request(
                    "`operationName` debe ser una cadena",
                    instance,
                )));
            }
        };
        let variables = match object.get("variables") {
            None | Some(Value::Null) => None,
            Some(value @ Value::Object(_)) => Some(value.clone()),
            Some(_) => {
                return GraphqlHttpResponse::Json(no_store(bad_request(
                    "`variables` debe ser un objeto JSON",
                    instance,
                )));
            }
        };

        let request = GraphqlRequest {
            query: query.to_owned(),
            operation_name,
            variables,
        };
        let prepared = match crate::validate::analyze(&self.schema, &request, self.config.max_depth)
        {
            Ok(prepared) => prepared,
            Err(errors) => {
                return GraphqlHttpResponse::Json(no_store(json_response(
                    StatusCode::OK,
                    &error_value(errors),
                )));
            }
        };
        log_operation(&prepared);

        let ctx = GraphqlContext::from_request(&req);
        if prepared.kind == OperationKind::Subscription {
            if !accepts_event_stream(req.headers()) {
                return GraphqlHttpResponse::Json(no_store(problem(
                    StatusCode::NOT_ACCEPTABLE,
                    "Not Acceptable",
                    "una suscripción exige `Accept: text/event-stream`",
                    instance,
                    None,
                )));
            }
            return match crate::execute::subscribe(&self.schema, &ctx, prepared).await {
                Ok(stream) => GraphqlHttpResponse::Sse(SseStream::start(stream, &self.shutdown)),
                Err(err) => GraphqlHttpResponse::Json(no_store(error_response_for(&err, instance))),
            };
        }

        GraphqlHttpResponse::Json(no_store(json_response(
            StatusCode::OK,
            &crate::execute::execute(&self.schema, &ctx, prepared).await,
        )))
    }
}

/// Respuesta del motor: JSON, HTML del explorer o cuerpo SSE (`RF-233`).
pub enum GraphqlHttpResponse {
    /// 200 con `{data, errors}`, o 4xx de transporte.
    Json(Response),
    /// 200 `text/html`: explorer.
    Html(Response),
    /// 200 `text/event-stream`: suscripción.
    Sse(SseStream),
}

/// Eventos ya enmarcados en SSE; la plataforma solo los bombea al cuerpo (`RP-031`).
pub struct SseStream {
    receiver: mpsc::Receiver<Bytes>,
}

impl SseStream {
    /// Solo la cola; quien use esto pone sus propias cabeceras.
    pub fn into_receiver(self) -> mpsc::Receiver<Bytes> {
        self.receiver
    }

    fn start(mut stream: GraphqlStream<Value>, shutdown: &CancellationToken) -> Self {
        let (tx, receiver) = mpsc::channel(64);
        let shutdown = shutdown.clone();

        tokio::spawn(async move {
            // El primer latido sale de inmediato: fija las cabeceras y confirma
            // al cliente que el flujo está abierto antes del primer evento.
            if !send(&tx, ": ping\n\n").await {
                return;
            }
            let mut ping = tokio::time::interval(PING);
            ping.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            ping.tick().await;

            loop {
                tokio::select! {
                    _ = shutdown.cancelled() => {
                        send(&tx, "event: complete\ndata: {}\n\n").await;
                        break;
                    }
                    _ = ping.tick() => {
                        if !send(&tx, ": ping\n\n").await {
                            break;
                        }
                    }
                    item = stream.recv() => match item {
                        Some(payload) => {
                            let body = serde_json::to_string(&payload)
                                .unwrap_or_else(|_| "{\"errors\":[{\"message\":\"error interno\"}]}".to_owned());
                            if !send(&tx, &format!("event: next\ndata: {body}\n\n")).await {
                                break;
                            }
                        }
                        None => {
                            send(&tx, "event: complete\ndata: {}\n\n").await;
                            break;
                        }
                    },
                }
            }
        });

        Self { receiver }
    }
}

async fn send(tx: &mpsc::Sender<Bytes>, event: &str) -> bool {
    tx.send(Bytes::from(event.to_owned())).await.is_ok()
}

/// Tipo de la operación seleccionada, sin validar el resto del documento.
fn select_kind(request: &GraphqlRequest) -> Result<OperationKind, Vec<GraphqlError>> {
    let document = crate::parser::parse(&request.query)
        .map_err(|err| vec![GraphqlError::at(err.message, err.position)])?;
    let operation =
        select_operation(&document, request.operation_name.as_deref()).map_err(|err| vec![err])?;
    Ok(operation.kind)
}

/// Registra la operación y sus campos raíz, nunca el documento (`RF-234`).
fn log_operation(prepared: &Prepared) {
    let roots: Vec<&str> = prepared
        .selection
        .fields
        .iter()
        .map(|field| field.name.as_str())
        .collect();
    tracing::debug!(
        operation = prepared.kind.keyword(),
        roots = ?roots,
        "operación GraphQL"
    );
}

fn explorer_response() -> Response {
    let mut resp = Response::new(Bytes::from(EXPLORER));
    resp.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    resp.headers_mut().insert(
        HeaderName::from_static("content-security-policy"),
        HeaderValue::from_static(
            "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; \
             connect-src 'self'",
        ),
    );
    resp.headers_mut().insert(
        HeaderName::from_static("x-content-type-options"),
        HeaderValue::from_static("nosniff"),
    );
    resp
}

/// RF-234: toda respuesta de `/graphql` es `no-store`.
fn no_store(mut resp: Response) -> Response {
    resp.headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    resp
}

fn problem(
    status: StatusCode,
    title: &str,
    detail: &str,
    instance: &str,
    allow: Option<&str>,
) -> Response {
    let body = Problem {
        r#type: "about:blank".to_owned(),
        title: title.to_owned(),
        status: status.as_u16(),
        detail: detail.to_owned(),
        instance: instance.to_owned(),
    };
    problem_response(status, &body, allow.map(str::to_owned))
}

fn bad_request(detail: &str, instance: &str) -> Response {
    problem(
        StatusCode::BAD_REQUEST,
        "Bad Request",
        detail,
        instance,
        None,
    )
}

fn is_json_content(headers: &HeaderMap) -> bool {
    header_str(headers, CONTENT_TYPE)
        .and_then(|value| value.split(';').next())
        .is_some_and(|essence| essence.trim().eq_ignore_ascii_case("application/json"))
}

fn accepts_event_stream(headers: &HeaderMap) -> bool {
    headers
        .get_all(ACCEPT)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .any(|item| {
            item.split(';')
                .next()
                .is_some_and(|essence| essence.trim().eq_ignore_ascii_case("text/event-stream"))
        })
}

fn header_str(headers: &HeaderMap, name: HeaderName) -> Option<&str> {
    headers.get(name).and_then(|value| value.to_str().ok())
}

fn query_params(uri: &Uri) -> Result<HashMap<String, String>, ()> {
    let mut params = HashMap::new();
    let Some(query) = uri.query() else {
        return Ok(params);
    };
    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        params.insert(decode_component(key)?, decode_component(value)?);
    }
    Ok(params)
}

/// Decodifica `%XX` y `+` de un componente de URL (`RF-234`).
fn decode_component(input: &str) -> Result<String, ()> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                out.push(b' ');
                index += 1;
            }
            b'%' => {
                let high = hex(bytes.get(index + 1).copied().ok_or(())?)?;
                let low = hex(bytes.get(index + 2).copied().ok_or(())?)?;
                out.push((high << 4) | low);
                index += 3;
            }
            other => {
                out.push(other);
                index += 1;
            }
        }
    }
    String::from_utf8(out).map_err(|_| ())
}

fn hex(byte: u8) -> Result<u8, ()> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(()),
    }
}

const EXPLORER: &str = include_str!("explorer.html");
