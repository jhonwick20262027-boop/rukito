//! Transporte MCP sobre Streamable HTTP (§10.8, ADR-23, ADR-24).
//!
//! Esta pieza no conoce ningún servidor: habla `rdd_core::http::{Request,
//! Response}` y entrega el SSE como una cola de `Bytes`. El montaje en la
//! plataforma (RP-029) lee el cuerpo con límite, ejecuta `HttpApp::prepare` y
//! vuelca esa cola en el cuerpo de la respuesta.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bytes::Bytes;
use rdd_core::Problem;
use rdd_core::http::response::error_response_for;
use rdd_core::http::{
    Extensions, HeaderMap, HeaderName, HeaderValue, Request, Response, StatusCode, header,
    json_response, normalize_path, problem_response,
};
use serde_json::{Value, json};
use tokio::sync::{Semaphore, mpsc};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::McpServer;
use crate::jsonrpc::{self, INTERNAL_ERROR, INVALID_REQUEST, METHOD_NOT_FOUND, PARSE_ERROR};
use crate::protocol;
use crate::tool::ToolContext;

/// Ruta por defecto del montaje (RF-222).
pub const DEFAULT_PATH: &str = "/mcp";
/// Sesiones simultáneas por defecto (RF-223).
pub const DEFAULT_MAX_SESSIONS: usize = 1024;
/// Caducidad de una sesión sin uso (RF-223).
pub const DEFAULT_SESSION_TTL: Duration = Duration::from_secs(30 * 60);
/// `tools/call` simultáneas por sesión (RF-229).
pub const DEFAULT_MAX_IN_FLIGHT: usize = 64;

const SESSION_HEADER: &str = "mcp-session-id";
const VERSION_HEADER: &str = "mcp-protocol-version";
const SSE_QUEUE: usize = 16;

type AuthorizeFn = Arc<dyn Fn(&Request) -> rdd_core::Result<()> + Send + Sync>;

/// Configuración del montaje `/mcp` (RF-222 a RF-229).
#[derive(Clone)]
pub struct McpHttpConfig {
    path: String,
    allowed_origins: Vec<String>,
    max_sessions: usize,
    session_ttl: Duration,
    max_in_flight: usize,
    authorize: Option<AuthorizeFn>,
}

impl Default for McpHttpConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl McpHttpConfig {
    /// `/mcp`, ningún `Origin`, 1024 sesiones, 30 min y 64 en vuelo.
    pub fn new() -> Self {
        Self {
            path: DEFAULT_PATH.to_owned(),
            allowed_origins: Vec::new(),
            max_sessions: DEFAULT_MAX_SESSIONS,
            session_ttl: DEFAULT_SESSION_TTL,
            max_in_flight: DEFAULT_MAX_IN_FLIGHT,
            authorize: None,
        }
    }

    /// Ruta del montaje; por defecto `/mcp` (RF-222).
    pub fn path(mut self, path: impl Into<String>) -> Self {
        self.path = path.into();
        self
    }

    /// Orígenes de navegador aceptados; vacío = solo peticiones sin `Origin`.
    pub fn allowed_origins<I, S>(mut self, origins: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.allowed_origins = origins.into_iter().map(Into::into).collect();
        self
    }

    /// Sesiones simultáneas antes de responder 503 (RF-223).
    pub fn max_sessions(mut self, max: usize) -> Self {
        self.max_sessions = max.max(1);
        self
    }

    /// Tiempo sin uso tras el que una sesión caduca (RF-223).
    pub fn session_ttl(mut self, ttl: Duration) -> Self {
        self.session_ttl = ttl;
        self
    }

    /// `tools/call` simultáneas por sesión (RF-229).
    pub fn max_in_flight(mut self, max: usize) -> Self {
        self.max_in_flight = max.max(1);
        self
    }

    /// Guardián que corre después de los globales y antes del protocolo.
    ///
    /// Un `Err` se responde con el mapeo del núcleo (Problem Details); a
    /// diferencia de un middleware, este no envuelve la respuesta.
    pub fn authorize<F>(mut self, f: F) -> Self
    where
        F: Fn(&Request) -> rdd_core::Result<()> + Send + Sync + 'static,
    {
        self.authorize = Some(Arc::new(f));
        self
    }
}

/// Resultado del protocolo listo para que la plataforma lo sirva (RP-029).
pub enum McpHttpResponse {
    /// 200 con JSON-RPC, o el 4xx del protocolo (415, 406, 503…).
    Json(Response),
    /// 202: era una notificación.
    Accepted,
    /// 204: `DELETE` o `tools/call` cancelada.
    NoContent,
    /// 200 `text/event-stream`; la plataforma bombea la cola al cuerpo.
    Sse(SseStream),
}

/// Eventos ya enmarcados en SSE.
///
/// La plataforma solo tiene que convertir el receptor en un `Stream<Bytes>` y
/// adjuntar las cabeceras; el protocolo y el framing ya están hechos.
pub struct SseStream {
    headers: HeaderMap,
    rx: mpsc::Receiver<Bytes>,
}

impl SseStream {
    /// Cabeceras de la respuesta SSE (`Content-Type`, `Cache-Control`, versión).
    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }

    /// Cabeceras y cola, para montar el cuerpo en streaming (RP-029).
    pub fn into_parts(self) -> (HeaderMap, mpsc::Receiver<Bytes>) {
        (self.headers, self.rx)
    }

    /// Solo la cola; quien use esto pone sus propias cabeceras.
    pub fn into_receiver(self) -> mpsc::Receiver<Bytes> {
        self.rx
    }
}

/// Semántica del transporte HTTP, neutral respecto al servidor (ADR-23).
pub struct McpHttp {
    server: Arc<McpServer>,
    config: McpHttpConfig,
    sessions: Mutex<HashMap<String, Arc<Session>>>,
    shutdown: CancellationToken,
}

impl McpHttp {
    /// Transporte sobre un `McpServer` ya construido.
    pub fn new(server: McpServer) -> Self {
        Self {
            server: Arc::new(server),
            config: McpHttpConfig::new(),
            sessions: Mutex::new(HashMap::new()),
            shutdown: CancellationToken::new(),
        }
    }

    /// Sustituye la configuración por defecto.
    pub fn config(mut self, config: McpHttpConfig) -> Self {
        self.config = config;
        self
    }

    /// Ruta donde la plataforma debe montarlo (RF-222).
    pub fn path(&self) -> &str {
        &self.config.path
    }

    /// Configuración efectiva, para que la plataforma valide el montaje.
    pub fn settings(&self) -> &McpHttpConfig {
        &self.config
    }

    /// Atiende una petición dirigida a la ruta del montaje.
    ///
    /// `req` ya pasó por `HttpApp::prepare` (RP-028): trae el contenedor y las
    /// extensiones que dejó la cadena global, que se copian a cada
    /// `ToolContext` (RF-228).
    pub async fn handle(&self, req: Request) -> McpHttpResponse {
        let path = normalize_path(req.uri().path());

        if !self.origin_allowed(req.headers()) {
            return self.problem(
                StatusCode::FORBIDDEN,
                "el origen de la petición no está permitido para /mcp",
                &path,
            );
        }

        if let Some(authorize) = &self.config.authorize
            && let Err(err) = authorize(&req)
        {
            return McpHttpResponse::Json(with_common_headers(
                error_response_for(&err, &path),
                None,
            ));
        }

        match req.method().as_str() {
            "POST" => self.post(req).await,
            "DELETE" => self.delete(&req, &path),
            _ => method_not_allowed(&path),
        }
    }

    /// Cancela lo que esté en vuelo y olvida las sesiones (RF-229).
    ///
    /// Un cliente que reconecte recibe 404 y reinicializa.
    pub async fn shutdown(&self) {
        self.shutdown.cancel();
        let sessions: Vec<Arc<Session>> = match self.sessions.lock() {
            Ok(mut map) => map.drain().map(|(_, session)| session).collect(),
            Err(_) => Vec::new(),
        };
        for session in sessions {
            session.cancel_all();
        }
    }

    async fn post(&self, req: Request) -> McpHttpResponse {
        let path = normalize_path(req.uri().path());

        if !is_json_content(req.headers()) {
            return self.problem(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "el cuerpo debe declarar `Content-Type: application/json`",
                &path,
            );
        }

        let (accepts_json, accepts_sse) = accepts(req.headers());
        if !accepts_json && !accepts_sse {
            return self.problem(
                StatusCode::NOT_ACCEPTABLE,
                "`Accept` debe incluir `application/json` o `text/event-stream`",
                &path,
            );
        }

        let value: Value = match serde_json::from_slice(req.body()) {
            Ok(value) => value,
            Err(_) => {
                return McpHttpResponse::Json(with_common_headers(
                    json_response(
                        StatusCode::BAD_REQUEST,
                        &jsonrpc::error(Value::Null, PARSE_ERROR, "JSON inválido"),
                    ),
                    None,
                ));
            }
        };

        if value.is_array() {
            return McpHttpResponse::Json(with_common_headers(
                json_response(
                    StatusCode::BAD_REQUEST,
                    &jsonrpc::error(
                        Value::Null,
                        INVALID_REQUEST,
                        "no se admite batching en /mcp",
                    ),
                ),
                None,
            ));
        }

        let request = match jsonrpc::decode(&value) {
            Ok(request) => request,
            Err(err) => {
                return McpHttpResponse::Json(with_common_headers(
                    json_response(
                        StatusCode::OK,
                        &jsonrpc::error(err.id, err.code, &err.message),
                    ),
                    None,
                ));
            }
        };

        let session_id = header_str(req.headers(), SESSION_HEADER).map(str::to_owned);
        let Some(session_id) = session_id else {
            return self.initialize_new(request, &path).await;
        };

        let Some(session) = self.live_session(&session_id) else {
            return self.problem(
                StatusCode::NOT_FOUND,
                "la sesión MCP no existe o expiró: vuelve a inicializar",
                &path,
            );
        };

        if let Some(version) = header_str(req.headers(), VERSION_HEADER)
            && (version != session.version.as_str() || !protocol::HTTP_VERSIONS.contains(&version))
        {
            return self.problem(
                StatusCode::BAD_REQUEST,
                &format!(
                    "versión `{version}` no servida por HTTP; se admiten {}",
                    protocol::HTTP_VERSIONS.join(", ")
                ),
                &path,
            );
        }

        if request.id.is_none() {
            if request.method == "notifications/cancelled" {
                session.cancel(&request.params);
            }
            return McpHttpResponse::Accepted;
        }

        let Some(id) = request.id.clone() else {
            return McpHttpResponse::NoContent;
        };
        let extensions = req.extensions().clone();

        match request.method.as_str() {
            "initialize" => McpHttpResponse::Json(with_common_headers(
                json_response(
                    StatusCode::OK,
                    &jsonrpc::error(id, INVALID_REQUEST, "`initialize` ya se procesó"),
                ),
                Some(&session),
            )),
            "ping" => McpHttpResponse::Json(with_common_headers(
                json_response(StatusCode::OK, &jsonrpc::response(id, json!({}))),
                Some(&session),
            )),
            "tools/list" => McpHttpResponse::Json(with_common_headers(
                json_response(
                    StatusCode::OK,
                    &jsonrpc::response(id, protocol::tools_list(&self.server)),
                ),
                Some(&session),
            )),
            "tools/call" => {
                self.call(session, id, request.params, accepts_sse, extensions)
                    .await
            }
            other => McpHttpResponse::Json(with_common_headers(
                json_response(
                    StatusCode::OK,
                    &jsonrpc::error(
                        id,
                        METHOD_NOT_FOUND,
                        &format!("método no encontrado: {other}"),
                    ),
                ),
                Some(&session),
            )),
        }
    }

    /// `initialize` sin sesión: crea una, responde en JSON y devuelve la cabecera
    /// `Mcp-Session-Id` (RF-223, RF-225).
    async fn initialize_new(&self, request: jsonrpc::Request, path: &str) -> McpHttpResponse {
        let Some(id) = request.id.clone() else {
            return self.problem(
                StatusCode::BAD_REQUEST,
                "`initialize` debe ser una petición con id y traer una sesión en lo sucesivo",
                path,
            );
        };
        if request.method != "initialize" {
            return self.problem(
                StatusCode::BAD_REQUEST,
                "falta la cabecera `Mcp-Session-Id`",
                path,
            );
        }

        let result = match protocol::initialize_with(
            &self.server,
            Some(&id),
            &request.params,
            protocol::HTTP_VERSIONS,
        ) {
            Ok(result) => result,
            Err(err) => {
                return McpHttpResponse::Json(with_common_headers(
                    json_response(
                        StatusCode::OK,
                        &jsonrpc::error(err.id, err.code, &err.message),
                    ),
                    None,
                ));
            }
        };
        let version = result
            .get("protocolVersion")
            .and_then(Value::as_str)
            .unwrap_or(protocol::HTTP_VERSIONS[0])
            .to_owned();

        let Some(session) = self.create_session(version) else {
            return self.problem(
                StatusCode::SERVICE_UNAVAILABLE,
                "se alcanzó el máximo de sesiones MCP simultáneas",
                path,
            );
        };

        let mut resp = with_common_headers(
            json_response(StatusCode::OK, &jsonrpc::response(id, result)),
            Some(&session),
        );
        if let Ok(value) = HeaderValue::from_str(&session.id) {
            resp.headers_mut()
                .insert(HeaderName::from_static(SESSION_HEADER), value);
        }
        tracing::debug!(version = %session.version, "sesión MCP HTTP creada");
        McpHttpResponse::Json(resp)
    }

    fn delete(&self, req: &Request, path: &str) -> McpHttpResponse {
        let Some(session_id) = header_str(req.headers(), SESSION_HEADER) else {
            return self.problem(
                StatusCode::BAD_REQUEST,
                "falta la cabecera `Mcp-Session-Id`",
                path,
            );
        };

        let removed = match self.sessions.lock() {
            Ok(mut map) => map.remove(session_id),
            Err(_) => None,
        };
        let Some(session) = removed else {
            return self.problem(
                StatusCode::NOT_FOUND,
                "la sesión MCP no existe o expiró",
                path,
            );
        };
        session.cancel_all();
        McpHttpResponse::NoContent
    }

    async fn call(
        &self,
        session: Arc<Session>,
        id: Value,
        params: Value,
        accepts_sse: bool,
        extensions: Extensions,
    ) -> McpHttpResponse {
        let call = match protocol::parse_call(&self.server, Some(&id), &params) {
            Ok(call) => call,
            Err(err) => {
                return McpHttpResponse::Json(with_common_headers(
                    json_response(
                        StatusCode::OK,
                        &jsonrpc::error(err.id, err.code, &err.message),
                    ),
                    Some(&session),
                ));
            }
        };

        if call.progress_token.is_some() && !accepts_sse {
            return self.problem(
                StatusCode::NOT_ACCEPTABLE,
                "`progressToken` exige `Accept: text/event-stream`",
                &normalize_path(&self.config.path),
            );
        }

        let Ok(permit) = Arc::clone(&session.slots).try_acquire_owned() else {
            return McpHttpResponse::Json(with_common_headers(
                json_response(
                    StatusCode::OK,
                    &jsonrpc::error(id, INTERNAL_ERROR, "demasiadas tools en vuelo"),
                ),
                Some(&session),
            ));
        };

        let key = serde_json::to_string(&id).unwrap_or_default();
        let cancel = CancellationToken::new();
        session.register(&key, cancel.clone());
        tracing::debug!(tool = call.tool.name(), "tools/call por HTTP");

        if call.progress_token.is_some() {
            self.call_sse(session, id, call, cancel, key, permit, extensions)
                .await
        } else {
            self.call_json(session, id, call, cancel, key, permit, extensions)
                .await
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn call_json(
        &self,
        session: Arc<Session>,
        id: Value,
        call: protocol::CallRequest,
        cancel: CancellationToken,
        key: String,
        permit: tokio::sync::OwnedSemaphorePermit,
        extensions: Extensions,
    ) -> McpHttpResponse {
        let (out, _) = mpsc::unbounded_channel();
        let ctx = ToolContext::new(None, cancel.clone(), out, extensions);
        let mut inner = tokio::spawn(async move { call.tool.call(call.args, ctx).await });

        let outcome = tokio::select! {
            joined = &mut inner => Some(joined),
            _ = cancel.cancelled() => {
                inner.abort();
                None
            }
        };

        session.unregister(&key);
        drop(permit);

        match outcome {
            // RF-227: la petición cancelada no entrega mensaje.
            None => McpHttpResponse::NoContent,
            Some(Ok(Ok(result))) => McpHttpResponse::Json(with_common_headers(
                json_response(
                    StatusCode::OK,
                    &jsonrpc::response(id, protocol::tool_ok(result)),
                ),
                Some(&session),
            )),
            Some(Ok(Err(error))) => McpHttpResponse::Json(with_common_headers(
                json_response(
                    StatusCode::OK,
                    &jsonrpc::response(id, protocol::tool_error(&error)),
                ),
                Some(&session),
            )),
            Some(Err(join)) => {
                tracing::error!(error = %join, "la tool terminó sin respuesta");
                McpHttpResponse::Json(with_common_headers(
                    json_response(
                        StatusCode::OK,
                        &jsonrpc::error(id, INTERNAL_ERROR, "la tool entró en pánico"),
                    ),
                    Some(&session),
                ))
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn call_sse(
        &self,
        session: Arc<Session>,
        id: Value,
        call: protocol::CallRequest,
        cancel: CancellationToken,
        key: String,
        permit: tokio::sync::OwnedSemaphorePermit,
        extensions: Extensions,
    ) -> McpHttpResponse {
        let (tx, rx) = mpsc::channel::<Bytes>(SSE_QUEUE);
        let headers = sse_headers(&session);
        let session_for_task = Arc::clone(&session);

        tokio::spawn(async move {
            let (out, mut out_rx) = mpsc::unbounded_channel();
            let ctx =
                ToolContext::new(call.progress_token.clone(), cancel.clone(), out, extensions);
            let mut inner = tokio::spawn(async move { call.tool.call(call.args, ctx).await });
            let mut final_message = None;

            loop {
                tokio::select! {
                    joined = &mut inner => {
                        // El contexto se soltó al terminar la tool; lo que quede
                        // encolado son progresos que van antes del resultado.
                        while let Ok(value) = out_rx.try_recv() {
                            if send_sse(&tx, value).await.is_err() {
                                cancel.cancel();
                            }
                        }
                        final_message = Some(match joined {
                            Ok(Ok(result)) => jsonrpc::response(id.clone(), protocol::tool_ok(result)),
                            Ok(Err(error)) => jsonrpc::response(id.clone(), protocol::tool_error(&error)),
                            Err(join) => {
                                tracing::error!(error = %join, "la tool terminó sin respuesta");
                                jsonrpc::error(id.clone(), INTERNAL_ERROR, "la tool entró en pánico")
                            }
                        });
                        break;
                    }
                    maybe = out_rx.recv() => match maybe {
                        Some(value) => {
                            if send_sse(&tx, value).await.is_err() {
                                // El cliente cerró: se cancela la tool (RF-226).
                                cancel.cancel();
                                inner.abort();
                                break;
                            }
                        }
                        None => {
                            let joined = (&mut inner).await;
                            final_message = Some(match joined {
                                Ok(Ok(result)) => jsonrpc::response(id.clone(), protocol::tool_ok(result)),
                                Ok(Err(error)) => jsonrpc::response(id.clone(), protocol::tool_error(&error)),
                                Err(join) => {
                                    tracing::error!(error = %join, "la tool terminó sin respuesta");
                                    jsonrpc::error(id.clone(), INTERNAL_ERROR, "la tool entró en pánico")
                                }
                            });
                            break;
                        }
                    },
                    _ = cancel.cancelled() => {
                        inner.abort();
                        break;
                    }
                }
            }

            if let Some(message) = final_message {
                let _ = send_sse(&tx, message).await;
            }
            session_for_task.unregister(&key);
            drop(permit);
            // Al soltar `tx` termina el flujo SSE (RF-226).
        });

        McpHttpResponse::Sse(SseStream { headers, rx })
    }

    fn origin_allowed(&self, headers: &HeaderMap) -> bool {
        let Some(origin) = headers.get(header::ORIGIN) else {
            return true;
        };
        let Ok(origin) = origin.to_str() else {
            return false;
        };
        self.config
            .allowed_origins
            .iter()
            .any(|allowed| allowed == origin)
    }

    /// Purgar caducadas en cada acceso y devolver la sesión viva (RF-223).
    fn live_session(&self, id: &str) -> Option<Arc<Session>> {
        let mut map = self.sessions.lock().ok()?;
        let ttl = self.config.session_ttl;
        map.retain(|_, session| session.last_used().is_some_and(|last| last.elapsed() < ttl));
        let session = map.get(id).cloned()?;
        session.touch();
        Some(session)
    }

    fn create_session(&self, version: String) -> Option<Arc<Session>> {
        let mut map = self.sessions.lock().ok()?;
        let ttl = self.config.session_ttl;
        map.retain(|_, session| session.last_used().is_some_and(|last| last.elapsed() < ttl));
        if map.len() >= self.config.max_sessions {
            return None;
        }
        let id = Uuid::new_v4().to_string();
        let session = Arc::new(Session::new(id.clone(), version, self.config.max_in_flight));
        map.insert(id, Arc::clone(&session));
        Some(session)
    }

    fn problem(&self, status: StatusCode, detail: &str, path: &str) -> McpHttpResponse {
        let problem = Problem {
            r#type: "about:blank".to_owned(),
            title: status.canonical_reason().unwrap_or("Error").to_owned(),
            status: status.as_u16(),
            detail: detail.to_owned(),
            instance: normalize_path(path),
        };
        McpHttpResponse::Json(with_common_headers(
            problem_response(status, &problem, None),
            None,
        ))
    }
}

/// Estado de una sesión viva; las peticiones en vuelo se registran por sesión y
/// no por conexión (RF-227).
struct Session {
    id: String,
    version: String,
    last_used: Mutex<Instant>,
    in_flight: Mutex<HashMap<String, CancellationToken>>,
    slots: Arc<Semaphore>,
}

impl Session {
    fn new(id: String, version: String, max_in_flight: usize) -> Self {
        Self {
            id,
            version,
            last_used: Mutex::new(Instant::now()),
            in_flight: Mutex::new(HashMap::new()),
            slots: Arc::new(Semaphore::new(max_in_flight)),
        }
    }

    fn last_used(&self) -> Option<Instant> {
        self.last_used.lock().ok().map(|instant| *instant)
    }

    fn touch(&self) {
        if let Ok(mut last) = self.last_used.lock() {
            *last = Instant::now();
        }
    }

    fn register(&self, key: &str, cancel: CancellationToken) {
        if let Ok(mut map) = self.in_flight.lock() {
            map.insert(key.to_owned(), cancel);
        }
    }

    fn unregister(&self, key: &str) {
        if let Ok(mut map) = self.in_flight.lock() {
            map.remove(key);
        }
    }

    fn cancel(&self, params: &Value) {
        let Some(request_id) = params
            .get("requestId")
            .filter(|value| value.is_string() || value.is_number())
        else {
            return;
        };
        let key = serde_json::to_string(request_id).unwrap_or_default();
        if let Ok(map) = self.in_flight.lock()
            && let Some(token) = map.get(&key)
        {
            token.cancel();
        }
    }

    fn cancel_all(&self) {
        if let Ok(map) = self.in_flight.lock() {
            for token in map.values() {
                token.cancel();
            }
        }
    }
}

fn header_str<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name).and_then(|value| value.to_str().ok())
}

fn is_json_content(headers: &HeaderMap) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|media| media.trim().eq_ignore_ascii_case("application/json"))
}

/// Qué formatos de respuesta acepta el cliente. Sin `Accept` se asume JSON.
fn accepts(headers: &HeaderMap) -> (bool, bool) {
    let Some(value) = headers.get(header::ACCEPT).and_then(|v| v.to_str().ok()) else {
        return (true, false);
    };
    let mut json = false;
    let mut sse = false;
    for part in value.split(',') {
        let media = part
            .split(';')
            .next()
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase();
        match media.as_str() {
            "*/*" => {
                json = true;
                sse = true;
            }
            "application/json" | "application/*" => json = true,
            "text/event-stream" | "text/*" => sse = true,
            _ => {}
        }
    }
    (json, sse)
}

fn with_common_headers(mut resp: Response, session: Option<&Session>) -> Response {
    resp.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    if let Some(session) = session
        && let Ok(version) = HeaderValue::from_str(&session.version)
    {
        resp.headers_mut()
            .insert(HeaderName::from_static(VERSION_HEADER), version);
    }
    resp
}

fn sse_headers(session: &Session) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/event-stream"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    if let Ok(version) = HeaderValue::from_str(&session.version) {
        headers.insert(HeaderName::from_static(VERSION_HEADER), version);
    }
    headers
}

fn method_not_allowed(path: &str) -> McpHttpResponse {
    let problem = Problem {
        r#type: "about:blank".to_owned(),
        title: "Method Not Allowed".to_owned(),
        status: StatusCode::METHOD_NOT_ALLOWED.as_u16(),
        detail: "el endpoint /mcp solo admite POST y DELETE".to_owned(),
        instance: normalize_path(path),
    };
    McpHttpResponse::Json(with_common_headers(
        problem_response(
            StatusCode::METHOD_NOT_ALLOWED,
            &problem,
            Some("DELETE, POST".to_owned()),
        ),
        None,
    ))
}

/// Un mensaje por evento, con el JSON compacto en una sola línea (RF-226).
async fn send_sse(tx: &mpsc::Sender<Bytes>, value: Value) -> Result<(), ()> {
    let data = serde_json::to_string(&value).map_err(|_| ())?;
    let frame = Bytes::from(format!("event: message\ndata: {data}\n\n"));
    tx.send(frame).await.map_err(|_| ())
}
