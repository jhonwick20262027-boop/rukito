# RDD-SPEC addendum 0.3.0 — Streaming, WebSocket, Realtime y MCP

| Campo | Valor |
| --- | --- |
| Documento | Addendum a `RDD-SPEC.md` (0.2.0-draft) → lo lleva a **0.3.0-draft** |
| Destinatario | Agente de implementación |
| Estado | Borrador para aprobación |
| Idioma | Español (código e identificadores en inglés, como en el spec base) |

---

## 0. Cómo usar este documento

1. **`RDD-SPEC.md` sigue mandando.** Este addendum solo lo modifica donde lo dice explícitamente (§2). Todo lo demás del spec base se mantiene.
2. **Palabras normativas:** **DEBE** = obligatorio. **NO DEBE** = prohibido. **DEBERÍA** = recomendado, con excepción justificada. **PUEDE** = opcional.
3. **IDs nuevos:** `RF-2xx` núcleo (streaming), `RF-3xx` realtime, `RF-4xx` MCP, `RP-03x` plataforma, `RA-02x` arquitectura, `C-13…C-24` suite de conformidad, `ADR-16…20`, `V11…V15` verificaciones. Los commits y PRs referencian los IDs.
4. **Orden de trabajo:** las fases §3 → §6 se implementan **en orden**. No se inicia una fase sin que la anterior cumpla su criterio de aceptación.
5. **Cambios al diseño:** si la implementación revela que un requisito es inviable, NO se desvía en silencio: se corrige este documento (y el registro de cambios, §11) en la misma PR.
6. **Versiones de dependencias:** no se fijan aquí. Usar las últimas estables compatibles y fijarlas en `Cargo.lock`. Lo marcado ⚠️ se verifica contra la versión real (§9).
7. **Restricciones de código (heredadas):** sin `nightly` (RN-010), sin `unsafe` (RN-011), `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test --workspace` en verde, rutas absolutas `::rdd::...` en código generado (RN-012).

---

## 1. Estado de partida (lo que YA existe)

Workspace Rust (edición 2024, MSRV declarado 1.85, probado con 1.91) con la **Fase 0.1 parcial** del spec base:

```
crates/
├── rdd-core/            tipos http, RouteDef, HttpApp::dispatch (matchit), trait Platform,
│                        Middleware/Next/fn_middleware, App/Module, Error + Problem Details,
│                        extractores Json/Path (FromRequest), shutdown_signal()
├── rdd-axum/            AxumPlatform: rutas nativas, límite de cuerpo, 405 vía fallback,
│                        barra final, .layer() Tower, apagado con plazo de gracia
├── rdd/                 fachada: re-exporta rdd-core, feature `axum`, trait AppRun::run()
└── rdd-platform-tests/  suite de conformidad (trait Client; casos C-02..C-07, C-12) contra
                         `dispatch` y contra TCP real
examples/todo-app/       domain.rs / service.rs / routes.rs (RouteDef escritos a mano) / mod.rs
```

**Línea base verificada:** `cargo test --workspace` pasa (5 tests en `rdd-core/tests`, 2 de conformidad, 1 del ejemplo). `cargo tree -p rdd-core` no contiene `axum` ni `hyper` (RA-011).

**API existente que este addendum toca** (firmas actuales):

```rust
pub type Request  = http::Request<Bytes>;
pub type Response = http::Response<Bytes>;                 // CAMBIA en §3
pub type BoxHandler = Arc<dyn Fn(Request) -> BoxFuture<'static, Response> + Send + Sync>;
pub type ShutdownSignal = Shared<BoxFuture<'static, ()>>;
pub trait Platform: Send + 'static {
    fn name(&self) -> &'static str;
    fn serve(self, app: HttpApp, listener: TcpListener, shutdown: ShutdownSignal) -> BoxFuture<'static, Result<()>>;
}
RouteDef::new(method, path, |req| async { Response })      // + with_middleware(mw)
App::new().middleware(..).module(Module::new().routes([..])).run_on(platform, addr)
```

**Pendiente heredado de la Fase 0.1 (fuera de este addendum, no bloquea):** macros (`#[controller]`, etc.), extractores `Query`/`Header`/`Ext`, C-11, CLI. Este addendum NO depende de las macros: los adaptadores se escriben con `RouteDef` / `Module` como en `todo-app`.

---

## 2. Cambios al spec base

| Sección base | Cambio |
| --- | --- |
| §2.2 «Streaming de cuerpos…», «WebSockets» | Se **levantan parcialmente**: streaming de **respuesta**, SSE, WebSocket, realtime y MCP pasan a estar dentro del alcance (fases 0.1b–0.8). Streaming de **petición** sigue fuera. GraphQL y gRPC siguen fuera. |
| RP-002 | Pasa a: `Request = http::Request<Bytes>` (sin cambio) y `Response = http::Response<Body>`, con `Body` definido en §3.1. |
| ADR-12 | **Reemplazado** por ADR-16. |
| RF-100 (logger) | Se añade: para respuestas `Body::Stream`, el evento se emite al enviarse las cabeceras con `stream=true` y la latencia es hasta cabeceras. |
| RP-009 (apagado) | Se añade: los streams y sockets activos DEBEN cerrarse al dispararse `ShutdownSignal` (RF-205, RP-032). |
| §11.1 | Se añade: un middleware NO DEBE bufferizar un `Body::Stream` completo (RF-206). |
| §17 (`rdd check`) | Nueva regla RA-020 (§6.6). |

### Nuevos ADR

| ID | Decisión | Alternativas descartadas | Motivo |
| --- | --- | --- | --- |
| ADR-16 | La **respuesta** puede ser stream (`Body::Stream`); la **petición** sigue en memoria con límite. | Streaming bidireccional completo; seguir sin streaming. | MCP, SSE y realtime solo necesitan streaming de salida; la petición pequeña mantiene simple la interfaz de plataforma. |
| ADR-17 | WebSocket es una **capacidad de plataforma** anunciada con `Platform::capabilities()`, no parte del trait base obligatorio. | Obligar a toda plataforma. | El upgrade HTTP es específico de cada servidor (RP-011). |
| ADR-18 | `rdd-realtime` define un protocolo **propio** (sobre JSON), NO el protocolo Engine.IO/Socket.IO. | Reimplementar Socket.IO; depender de `socketioxide`. | Socket.IO es grande y cambia entre versiones. Un crate opcional `rdd-socketio` sobre `socketioxide` queda como candidato futuro (solo Axum). |
| ADR-19 | `rdd-mcp` implementa el protocolo MCP **a mano** sobre JSON-RPC 2.0, sin SDK, como adaptador de entrada compuesto de `RouteDef`. | SDK oficial de MCP; plataforma especial para MCP. | Mantiene la independencia de plataforma y reutiliza `Module`, middlewares y el apagado ordenado. |
| ADR-20 | Las herramientas MCP son un **adaptador de entrada** en `<modulo>/mcp.rs` que llama a servicios, igual que `routes.rs`. | Poner tools en `domain.rs` o `service.rs`. | Preserva RA-001 (el dominio no conoce el transporte). |

---

## 3. Fase 0.1b — Streaming de respuesta en el núcleo

**Entrega:** `Body::Stream`, SSE, cancelación por desconexión, apagado de streams, backpressure, en `rdd-core` y `rdd-axum`.

### 3.1 Tipos

```rust
// rdd-core::http
pub type BoxError = Box<dyn std::error::Error + Send + Sync>;

pub enum Body {
    Full(Bytes),
    Stream(BoxStream<'static, Result<Bytes, BoxError>>),
}
impl From<Bytes> for Body {}  impl From<String> for Body {}  impl From<&'static str> for Body {}
impl Default for Body {}      // Full(vacío)
impl Body {
    pub fn as_bytes(&self) -> Option<&Bytes>;               // Some solo si Full
    pub async fn collect(self) -> Result<Bytes>;            // agota un Stream (solo tests/dispatch)
    pub fn from_stream<S>(s: S) -> Self where S: Stream<Item = Result<Bytes, BoxError>> + Send + 'static;
}
pub type Response = http::Response<Body>;
```

| ID | Requisito |
| --- | --- |
| RF-200 | `Response` DEBE ser `http::Response<Body>`. `Request` NO cambia. Todo código existente que construye respuestas (`problem_response`, `json_response`, `into_response`, `default_fallback`) DEBE devolver `Body::Full`. |
| RF-201 | Los tests y clientes que leían `resp.body()` como `Bytes` DEBEN migrar a `Body::as_bytes()` / `Body::collect()`. La línea base de §1 DEBE seguir en verde tras la migración. |
| RF-202 | `HttpApp::dispatch` DEBE devolver el `Body::Stream` sin consumirlo. |

### 3.2 SSE

```rust
pub struct Event { pub id: Option<String>, pub event: Option<String>, pub data: String, pub retry: Option<Duration> }
impl Event { pub fn data(s: impl Into<String>) -> Self; pub fn json<T: Serialize>(v: &T) -> Result<Self>; /* builders: id(), event() */ }

pub fn sse<S>(events: S) -> Response where S: Stream<Item = Event> + Send + 'static;
pub struct SseOptions { pub keep_alive: Duration /* por defecto 15 s */ }
pub fn sse_with<S>(events: S, opts: SseOptions) -> Response;
```

| ID | Requisito |
| --- | --- |
| RF-203 | `sse()` DEBE responder `200`, `Content-Type: text/event-stream`, `Cache-Control: no-cache`, y serializar cada `Event` según el formato `text/event-stream` (campos `id:`, `event:`, `data:` — un `data:` por línea —, `retry:`, línea en blanco final). Los saltos de línea dentro de `data` DEBEN dividirse en varios `data:`. |
| RF-204 | DEBE emitir un comentario de keep-alive (`: ping\n\n`) cada `keep_alive` sin eventos. |

### 3.3 Cancelación, apagado y backpressure

La plataforma DEBE insertar en `req.extensions()` un `Connection`:

```rust
pub struct Connection {
    pub closed:   CancellationToken,   // se cancela cuando el cliente se desconecta
    pub shutdown: ShutdownSignal,      // se dispara al apagar la app
}
impl Connection { pub async fn done(&self); /* resuelve con closed O shutdown */ }
```

| ID | Requisito |
| --- | --- |
| RF-205 | Un stream DEBE terminar (y liberar sus recursos) cuando `Connection::done()` resuelve. Los helpers `sse()`/`sse_with()` DEBEN aplicarlo automáticamente si la petición los habilita con `sse_for(&req, stream)`. |
| RF-206 | Los middlewares reciben el `Body::Stream` intacto; ninguno del núcleo ni de los crates oficiales DEBE bufferizarlo. `Next::run` NO cambia de firma. |
| RF-207 | Helper `stream_channel(capacity) -> (Sender<Event>, impl Stream<Item = Event>)` sobre `tokio::sync::mpsc` **acotado**. `capacity` es obligatoria (sin canales ilimitados). |
| RF-208 | `Sender` DEBE ofrecer `send(..).await` (espera) y `try_send(..)`. La política `Overflow::{Wait, Disconnect}` se elige al crear el canal; por defecto `Disconnect` (un consumidor lento no bloquea al productor). |
| RF-209 | Un `panic` o error DESPUÉS de enviadas las cabeceras NO puede cambiar el estado HTTP: DEBE terminar el stream y registrarse con `tracing::error!`. La captura de panic de RF-073 solo cubre hasta la creación de la `Response`. |

### 3.4 Plataforma

| ID | Requisito |
| --- | --- |
| RP-030 | `rdd-axum` DEBE convertir `Body::Stream` a un cuerpo de Axum en streaming, sin bufferizar. |
| RP-031 | La plataforma DEBE insertar `Connection` en cada petición y cancelar `closed` cuando el cliente cierre la conexión. |
| RP-032 | Al dispararse `ShutdownSignal`, la plataforma DEBE: dejar de aceptar conexiones, cancelar `shutdown` de las conexiones con streams activos, y esperar hasta `shutdown_grace`; pasado ese plazo, cerrar. |

### 3.5 Conformidad y aceptación

Extender `rdd-platform-tests` (el trait `Client` DEBE poder leer un stream por partes).

| Caso | Verifica | Req. |
| --- | --- | --- |
| C-13 | Un SSE de 3 eventos espaciados 100 ms llega **por eventos** (el primero antes de 250 ms), con `content-type: text/event-stream`. | RF-203, RP-030 |
| C-14 | Si el cliente se desconecta, el productor del stream se cancela en < 1 s (verificable con un contador/flag compartido). | RF-205, RP-031 |
| C-15 | Con un SSE abierto, al apagar la app el stream se cierra dentro de `shutdown_grace` y el servidor termina. | RP-032 |
| C-16 | Un middleware global que añade una cabecera la aplica a una respuesta stream sin bufferizarla (el primer evento llega antes del final). | RF-206 |
| C-17 | `stream_channel` con `Disconnect`: un consumidor que no lee provoca el cierre, no memoria ilimitada. | RF-208 |

**Aceptación 0.1b:** línea base de §1 en verde; C-13…C-17 pasan en `AxumPlatform` y, donde aplique (todos salvo C-14/C-15), en `dispatch`; el `todo-app` expone `GET /tasks/events` con SSE de ejemplo.

---

## 4. Fase 0.6 — WebSocket como capacidad de plataforma

```rust
pub struct Capabilities { pub streaming: bool, pub websocket: bool }
// en Platform: fn capabilities(&self) -> Capabilities { Capabilities { streaming: true, websocket: false } }

pub enum Message { Text(String), Binary(Bytes), Ping(Bytes), Pong(Bytes), Close(Option<CloseFrame>) }
pub struct WebSocket { /* Stream<Item=Result<Message>> + Sink<Message> */ }

/// Devuelve la Response que pide el upgrade; la plataforma completa el handshake (RFC 6455).
pub fn websocket<F, Fut>(req: &Request, handler: F) -> Result<Response>
where F: FnOnce(WebSocket) -> Fut + Send + 'static, Fut: Future<Output = ()> + Send + 'static;
```

| ID | Requisito |
| --- | --- |
| RF-250 | `websocket()` DEBE devolver `Error::BadRequest` (400) si la petición no es un upgrade válido, y `Error` 501 si la plataforma activa no soporta WebSocket (`capabilities().websocket == false`). Con `run_on` el núcleo DEBE registrar las capacidades al arrancar (RN-042). |
| RP-033 | Una plataforma que anuncie `websocket: true` DEBE: detectar la extensión de upgrade en la respuesta del handler, completar el handshake con la petición original, y ejecutar el `handler` con un `WebSocket` en una tarea propia. |
| RP-034 | El `WebSocket` DEBE heredar `Connection` (cancelación y apagado): al apagar, DEBE enviar `Close(1001)` y terminar dentro de `shutdown_grace`. |
| RP-035 | Mensajes entrantes mayores que `limits.ws_max_message_bytes` (por defecto 1 MiB, nuevo campo de `Limits`) DEBEN cerrar con código `1009`. |
| RP-036 | Ping/pong: la plataforma DEBE responder a `Ping` automáticamente y DEBERÍA enviar `Ping` cada 30 s a conexiones inactivas. |
| RA-021 | `rdd-axum` puede depender de `tokio-tungstenite`/`axum` (feature `ws`); `rdd-core` NO DEBE. |

| Caso | Verifica | Req. |
| --- | --- | --- |
| C-18 | Eco por WebSocket: texto y binario vuelven iguales. | RP-033 |
| C-19 | Upgrade inválido → 400; ruta sin upgrade → 404/405 normal. | RF-250 |
| C-20 | Mensaje > límite → cierre `1009`. | RP-035 |
| C-21 | Apagado con un WS abierto → `Close(1001)` y servidor terminado. | RP-034 |

**Aceptación 0.6:** C-18…C-21 pasan en `AxumPlatform`. Cliente de prueba: `tokio-tungstenite` como `dev-dependency` de `rdd-platform-tests`. `dispatch` NO soporta WebSocket (documentado, los casos se marcan `#[cfg]`/skip).

---

## 5. Fase 0.7 — `rdd-realtime` (estilo Socket.IO, protocolo propio)

Depende solo de `rdd-core` (RA-011). Transporte principal: WebSocket (§4). Fallback opcional: SSE (servidor→cliente) + `POST` (cliente→servidor).

### 5.1 Protocolo (JSON, un mensaje de texto = un objeto)

```json
{"event":"message","data":{"text":"hola"},"id":12}      // cliente→servidor, `id` pide ack
{"ack":12,"data":"ok"}                                    // respuesta a un ack
{"event":"message","data":{...}}                          // servidor→cliente
```

### 5.2 API

```rust
let rt = Realtime::new();                       // provider singleton (Hub)
rt.namespace("/chat")
  .middleware(Authenticated)                    // el Middleware del núcleo corre en el handshake
  .on_connect(|sock| async move { sock.join("general").await; })
  .on("message", |sock, msg: Msg, ack| async move {
      sock.to("general").emit("message", &msg).await?;
      ack.send("ok")
  })
  .on_disconnect(|sock| async move { /* limpieza */ });

Module::new().routes(rt.routes("/realtime"))    // monta GET /realtime/chat (upgrade)
```

`Socket`: `id()`, `emit(event, &T)`, `join(room)`, `leave(room)`, `to(room) -> Emitter`, `broadcast()` (a todos salvo el emisor), `disconnect()`, `extensions()` (datos puestos por middlewares).

| ID | Requisito |
| --- | --- |
| RF-300 | **Namespaces** aislados por ruta; **rooms** por namespace; un socket DEBE pertenecer implícitamente a la room con su propio `id`. |
| RF-301 | `emit` a una room DEBE entregar el mensaje a todos sus miembros conectados, en el orden de emisión por emisor. |
| RF-302 | **Ack:** si el mensaje trae `id`, el handler DEBE poder responder una vez con `ack.send(valor)`; un segundo `send` es un error. Si el handler termina sin responder, el cliente NO recibe ack (no se inventa uno). |
| RF-303 | Cada socket DEBE tener una **cola de salida acotada** (por defecto 256). Overflow → política configurable `Disconnect` (por defecto) o `DropOldest`. NO DEBE haber colas ilimitadas. |
| RF-304 | Evento desconocido → NO cierra el socket; se registra y se responde `{"error":...}` solo si traía `id`. JSON inválido o sin `event`/`ack` → cierre `1007`. |
| RF-305 | Al desconectarse un socket (cliente, error o apagado) DEBE salir de todas sus rooms y ejecutar `on_disconnect` exactamente una vez. |
| RF-306 | Un `panic` en un handler de evento DEBE capturarse, registrarse y NO cerrar el socket ni el servidor. |
| RF-307 | El estado compartido (rooms, sockets) DEBE protegerse sin mantener un lock durante un `.await`; usar canales o secciones críticas cortas. |
| RF-308 | Para escalar a varias instancias queda **fuera de alcance** (requeriría un bus). Documentar la limitación. |

| Caso | Verifica | Req. |
| --- | --- | --- |
| C-22 | Dos clientes en la misma room: A emite, B recibe y A no. Un tercero fuera de la room no recibe. | RF-300/301 |
| C-23 | Ack: el cliente recibe `{ack,data}` con el `id` correcto; evento sin `id` no genera ack. | RF-302 |
| C-24 | Consumidor lento con política `Disconnect` → se cierra; con `DropOldest` → recibe los mensajes más recientes. | RF-303 |

**Aceptación 0.7:** C-22…C-24 y tests unitarios de RF-305/306 pasan; ejemplo `todo-app` con un namespace `/tasks` que difunde "tarea creada" a todos los conectados.

---

## 6. Fase 0.8 — `rdd-mcp` (Model Context Protocol)

Crate nuevo; depende de `rdd-core` (y `schemars` para esquemas JSON, solo aquí). Es un **adaptador de entrada** (ADR-19, ADR-20): produce un `Module` con `RouteDef`s, sin tocar la plataforma. Requiere la fase 0.1b; los tools no requieren WebSocket.

### 6.1 API

```rust
let mcp = McpServer::builder()
    .name("todo-app").version("0.1.0")
    .tools(tasks::mcp::tools(svc))              // Vec<Tool> — adaptador del módulo (ver §6.6)
    .allowed_origins(["https://mi-cliente.com"]) // RF-420
    .session_ttl(Duration::from_secs(1800))
    .build();

App::new().module(mcp.http_module("/mcp")).run("0.0.0.0:3000").await   // Streamable HTTP
mcp.serve_stdio().await                                              // transporte stdio

Tool::new("create_task")
    .description("Crea una tarea")
    .input::<NewTask>()                         // `schemars::JsonSchema` + `Deserialize`
    .handler(|args: NewTask, ctx: McpCtx| async move { /* -> Result<ToolOutput> */ });
```

`McpCtx`: `session_id()`, `cancelled() -> CancellationToken` (RF-410), `progress(f64, Option<String>)` (RF-411), `notify_tools_changed()`.

### 6.2 Protocolo (JSON-RPC 2.0)

| ID | Requisito |
| --- | --- |
| RF-400 | DEBE implementar JSON-RPC 2.0: solicitudes (`id`), notificaciones (sin `id`, NO se responden) y respuestas. Errores estándar: `-32700` parse, `-32600` request inválida, `-32601` método no encontrado, `-32602` parámetros inválidos, `-32603` interno. |
| RF-401 | Métodos obligatorios: `initialize`, `notifications/initialized`, `ping`, `tools/list`, `tools/call`. Opcionales (DEBERÍA, ordenados por prioridad): `resources/list` + `resources/read`, `prompts/list` + `prompts/get`. |
| RF-402 | `initialize` DEBE negociar versión del protocolo: si la solicitada está soportada, responderla; si no, responder la más reciente soportada. Declarar solo las capabilities realmente implementadas (`tools`, con `listChanged` si aplica). ⚠️ V11 |
| RF-403 | `tools/list` DEBE devolver `name`, `description` e `inputSchema` (JSON Schema generado desde el tipo de entrada). |
| RF-404 | `tools/call`: argumentos inválidos → error JSON-RPC `-32602`. **Errores de dominio** (`rdd::Error`) NO son errores de protocolo: DEBEN devolverse como resultado con `isError: true` y el mensaje (para `Internal`, mensaje genérico, RF-041). |
| RF-405 | El resultado de un tool DEBE ser `content: [{type:"text", text}]`; con salida estructurada PUEDE añadir `structuredContent`. |
| RF-410 | `notifications/cancelled` DEBE cancelar el `CancellationToken` del `McpCtx` de la solicitud indicada. |
| RF-411 | Si la solicitud trae `progressToken`, `ctx.progress()` DEBE emitir `notifications/progress` por el stream de esa solicitud. |

### 6.3 Transporte Streamable HTTP ⚠️ V11

Un solo endpoint (`/mcp` por defecto).

| ID | Requisito |
| --- | --- |
| RF-412 | `POST` con un mensaje JSON-RPC. Debe exigir `Accept` que incluya `application/json` y `text/event-stream` (si no, 406). Solicitud → respuesta `application/json` **o** `text/event-stream` (SSE; DEBE usarse SSE cuando el handler emita progreso/notificaciones antes de responder). Notificación o respuesta del cliente → `202` sin cuerpo. |
| RF-413 | `initialize` DEBE crear una sesión y devolver la cabecera `Mcp-Session-Id` (UUID, solo ASCII visible). El resto de peticiones DEBE enviarla: ausente → `400`, desconocida/expirada → `404`. |
| RF-414 | DEBE validar la cabecera `MCP-Protocol-Version` en peticiones posteriores a `initialize`; valor no soportado → `400`. |
| RF-415 | `GET` abre un stream SSE de notificaciones servidor→cliente de la sesión. Si el servidor no lo soporta DEBE responder `405`. |
| RF-416 | `DELETE` con `Mcp-Session-Id` cierra la sesión y sus streams (`204`). |
| RF-417 | **Reanudación:** cada evento SSE de sesión DEBE llevar un `id` monótono por sesión; con `Last-Event-ID` el servidor DEBERÍA reenviar lo pendiente desde un buffer circular (por defecto 256 eventos). |
| RF-418 | Sesiones en `SessionStore` (trait) con implementación en memoria; TTL por inactividad (por defecto 30 min) y máximo de sesiones configurable. Provider singleton. |
| RF-419 | Cada sesión DEBE tener su propio canal de salida acotado (RF-207/208). |
| RF-420 | **Seguridad:** DEBE validar `Origin` contra una lista de permitidos; ausente = permitido (clientes no-navegador), presente y no permitido → `403`. Por defecto la lista está **vacía** (denegar orígenes de navegador). La autenticación se hace con `Middleware` normales aplicados al `Module` del MCP; el crate NO inventa un esquema de auth. |
| RF-421 | Límite de cuerpo del `POST` DEBE usar `limits.body_limit_bytes` (RF-017). |

### 6.4 Transporte stdio

| ID | Requisito |
| --- | --- |
| RF-430 | Mensajes JSON-RPC delimitados por salto de línea en `stdin`/`stdout`; los logs van SOLO a `stderr`. Un mensaje NO DEBE contener saltos de línea embebidos. |
| RF-431 | Una sola sesión implícita por proceso. Mismo registro de tools y misma lógica de protocolo que HTTP (el núcleo del protocolo DEBE ser independiente del transporte). |

### 6.5 Estructura interna de `rdd-mcp`

```
crates/rdd-mcp/src/
├── lib.rs          McpServer, builder
├── jsonrpc.rs      tipos y códigos de error (independiente del transporte)
├── protocol.rs     despacho de métodos (initialize, tools/*, ...) -> (Response | Stream)
├── tool.rs         Tool, ToolOutput, McpCtx, registro
├── session.rs      SessionStore (trait) + InMemorySessionStore
└── transport/
    ├── http.rs     RouteDefs POST/GET/DELETE
    └── stdio.rs
```

### 6.6 Ubicación en un proyecto DDD y regla de arquitectura

```
src/tasks/
├── domain.rs      service.rs      routes.rs (REST)
├── mcp.rs         ← adaptador MCP: `pub fn tools(svc: TaskService) -> Vec<Tool>`
└── mod.rs
```

| ID | Requisito |
| --- | --- |
| RA-020 | `rdd check` DEBE tratar `mcp.rs` igual que `routes.rs` (RA-003): NO DEBE importar `generated`, `sqlx` ni `Repo`; solo puede llamar a `service.rs`. Un módulo puede ofrecer REST, MCP o ambos. |
| RA-022 | `rdd-mcp` NO DEBE depender de `axum`, `hyper` ni de ninguna plataforma (RA-011). |

### 6.7 Conformidad y aceptación

Suite propia en `rdd-mcp/tests`, ejecutada contra `dispatch` y contra `AxumPlatform`.

| Caso | Verifica | Req. |
| --- | --- | --- |
| M-01 | `initialize` crea sesión y devuelve `Mcp-Session-Id`; negociación de versión. | RF-402/413 |
| M-02 | `tools/list` devuelve esquema válido; `tools/call` feliz. | RF-403/405 |
| M-03 | Error de dominio → `isError:true`; argumentos inválidos → `-32602`. | RF-404 |
| M-04 | Notificación → `202` sin cuerpo; método desconocido → `-32601`. | RF-400 |
| M-05 | Sin sesión → 400; sesión inexistente → 404; `DELETE` cierra y luego → 404. | RF-413/416 |
| M-06 | Tool con progreso responde por SSE con `notifications/progress` antes del resultado final. | RF-411/412 |
| M-07 | `notifications/cancelled` cancela un tool largo. | RF-410 |
| M-08 | `GET` recibe una notificación emitida por el servidor; reconexión con `Last-Event-ID` recupera lo perdido. | RF-415/417 |
| M-09 | `Origin` no permitido → 403; sin `Origin` → pasa. | RF-420 |
| M-10 | El mismo protocolo funciona por stdio (proceso hijo con tuberías). | RF-430/431 |

**Aceptación 0.8:** M-01…M-10 pasan; el `todo-app` expone `create_task`, `list_tasks` y `complete_task` por `/mcp` **y** por stdio; un cliente MCP real (p. ej. el inspector oficial) lista y ejecuta los tools. ⚠️ V11

---

## 7. Reglas de dependencias (extiende §4.2)

| Crate | Puede depender de | NO DEBE depender de |
| --- | --- | --- |
| `rdd-core` | además: `tokio-util` (CancellationToken), `futures` | `axum`, `hyper`, `tokio-tungstenite`, cualquier plataforma |
| `rdd-axum` | además: `tokio-tungstenite` (feature `ws`) | `rdd-hyper` |
| `rdd-realtime` | `rdd-core`, `serde`, `serde_json`, `tokio`, `tracing` | `axum`, `hyper`, `rdd-mcp` |
| `rdd-mcp` | `rdd-core`, `schemars`, `serde`, `serde_json`, `tokio`, `uuid`, `tracing` | `axum`, `hyper`, `rdd-realtime` |
| `rdd` (fachada) | features opcionales `realtime`, `mcp` | — |

CI DEBE seguir verificando RA-011 (`cargo tree` / `cargo-deny bans`) para `rdd-core`, `rdd-realtime` y `rdd-mcp`.

---

## 8. Criterios transversales

- [ ] `cargo fmt --check`, `cargo clippy --workspace -- -D warnings`, `cargo test --workspace` en verde en cada fase.
- [ ] Cada requisito DEBE con test de integración o de conformidad; casos nuevos de comportamiento HTTP van a la suite en la misma PR (RP-022).
- [ ] Los tests de streaming NO DEBEN usar `sleep` fijo largo: usar tiempos acotados (< 2 s) y señales explícitas.
- [ ] Ningún test deja procesos ni tareas colgadas (los servidores de prueba se apagan con `ShutdownSignal`).
- [ ] Documentación con ejemplo ejecutable por cada API pública nueva (`cargo test --doc`).
- [ ] Cada fase termina actualizando `RDD-SPEC.md` y el registro de cambios.

---

## 9. Verificaciones pendientes ⚠️

| # | Verificar | Fase | Impacto |
| --- | --- | --- | --- |
| V11 | Versión vigente de la especificación MCP y su transporte Streamable HTTP: cabeceras (`Mcp-Session-Id`, `MCP-Protocol-Version`), códigos de estado, reanudación con `Last-Event-ID`, lista de versiones soportables, si `initialize` admite lotes. Contrastar con la spec oficial antes de fijar RF-402/412–417. | 0.8 | RF-4xx |
| V12 | Cómo completar el upgrade WebSocket de forma que `rdd-core` siga neutral: extensión en `Response` + handshake en la plataforma (`axum::extract::ws` / `hyper::upgrade`). | 0.6 | RF-250, RP-033 |
| V13 | Cómo detectar la desconexión del cliente en el cuerpo en streaming de Axum (drop del cuerpo/stream) para cancelar `Connection::closed`. | 0.1b | RP-031 |
| V14 | Compatibilidad de `schemars` con la versión de `serde` y su soporte de JSON Schema para el `inputSchema` de MCP. | 0.8 | RF-403 |
| V15 | Si el `shutdown` con streams activos de `axum::serve` espera indefinidamente y cómo forzar el cierre tras `shutdown_grace`. | 0.1b | RP-032 |

---

## 10. Fuera de alcance de este addendum

Streaming de **petición** (subidas grandes), compatibilidad con clientes oficiales de Socket.IO (candidato `rdd-socketio`), escalado horizontal / bus entre instancias, autenticación OAuth del servidor MCP, `rdd-hyper` (sigue en su fase 0.5 del spec base, pero DEBERÁ implementar `capabilities()` y `Body::Stream` cuando exista), macros para declarar tools (`#[mcp_tool]`, candidato posterior).

---

## 11. Registro de cambios

| Versión | Fecha | Cambio |
| --- | --- | --- |
| 0.3.0-draft | 2026-11-07 | Streaming de respuesta (`Body::Stream`, SSE, cancelación, backpressure), WebSocket como capacidad de plataforma, `rdd-realtime` (protocolo propio) y `rdd-mcp` (Streamable HTTP + stdio). Sustituye ADR-12; nuevos ADR-16…20; nuevas verificaciones V11…V15. |

---

## 12. Orden de trabajo para el agente

1. Leer `RDD-SPEC.md` y este addendum completos; ejecutar `cargo test --workspace` y confirmar la línea base de §1.
2. **0.1b:** aplicar §3.1 (`Body`) y arreglar la migración (RF-201) → SSE (§3.2) → `Connection` + cancelación/apagado (§3.3–3.4) → casos C-13…C-17. Resolver V13 y V15 al empezar.
3. **0.6:** `Capabilities` + `websocket()` + implementación en `rdd-axum` → C-18…C-21. Resolver V12 primero.
4. **0.7:** `rdd-realtime` → C-22…C-24.
5. **0.8:** núcleo del protocolo independiente del transporte → transporte HTTP → stdio → M-01…M-10. **Resolver V11 antes de escribir el transporte HTTP.**
6. Tras cada fase: actualizar este documento si algo cambió, y dejar el workspace en verde antes de continuar.
