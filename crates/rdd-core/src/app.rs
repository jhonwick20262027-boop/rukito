//! `App`: el constructor de la aplicación (§6.1).

use std::any::{Any, TypeId};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use crate::db::{Db, DbAdapter, MemoryBackend};
use crate::di::{BoxedValue, ContainerBuilder, ContainerError, ProviderReg};
#[cfg(feature = "default-platform")]
use crate::http::platform::default_platform;
use crate::http::platform::{Platform, PlatformFuture, ShutdownSignal};
use crate::http::routes::{HttpApp, Limits, RouteDef, normalize_path, to_route_syntax};
use crate::http::types::{BoxHandler, Method, Middleware, Response, StatusCode};
use crate::module::{Controller, Module};
use crate::socket::{SocketHandler, UpgradeDef};

/// Límite de cuerpo por defecto: 2 MB (RF-017).
pub const DEFAULT_BODY_LIMIT: usize = 2 * 1024 * 1024;

/// Mensaje claro cuando se llama a `run()` sin plataforma por defecto (RP-012).
pub const NO_DEFAULT_PLATFORM: &str = "\
no hay plataforma por defecto instalada.
  - si compilaste `rdd` sin la feature `axum`, usa `.run_on(HyperPlatform::new(), addr)`;
  - con la feature `axum` activa, la fachada instala Axum automáticamente (RP-012).";

/// Backend de persistencia ya resuelto (RF-110, RF-111).
#[derive(Clone)]
pub enum Backend {
    /// `MemoryBackend` del núcleo: desarrollo y tests (RF-015, §20).
    Memory(Arc<MemoryBackend>),
    /// Motor externo conectado a través de su adaptador (§12).
    Adapter(Arc<dyn DbAdapter>),
}

impl Default for Backend {
    fn default() -> Self {
        Self::Memory(Arc::new(MemoryBackend::new()))
    }
}

impl Backend {
    /// Nombre legible para el registro de arranque (RN-042).
    pub fn label(&self) -> &'static str {
        match self {
            Self::Memory(_) => "memoria",
            Self::Adapter(adapter) => adapter.name(),
        }
    }

    /// El adaptador activo, sin importar el motor.
    pub fn adapter(&self) -> Arc<dyn DbAdapter> {
        match self {
            Self::Memory(memory) => Arc::clone(memory) as Arc<dyn DbAdapter>,
            Self::Adapter(adapter) => Arc::clone(adapter),
        }
    }
}

/// Configuración acumulada de la app (§15.1).
#[derive(Clone, Debug)]
pub struct AppConfig {
    /// Nombre del proyecto.
    pub name: String,
    /// Dirección de escucha configurada.
    pub host: String,
    /// Puerto configurado.
    pub port: u16,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            name: "rdd-app".to_owned(),
            host: "0.0.0.0".to_owned(),
            port: 3000,
        }
    }
}

/// Resultado de `App::build()`: la app HTTP lista para la plataforma (RF-012).
pub struct BuiltApp {
    /// La app HTTP compuesta.
    pub http: HttpApp,
    /// Backend de persistencia activo.
    pub backend: Backend,
    /// Contenedor resuelto.
    pub container: Arc<crate::di::Container>,
}

/// Aplicación `rdd`: módulos, middlewares globales, providers y configuración.
pub struct App {
    modules: Vec<Module>,
    globals: Vec<Arc<dyn Middleware>>,
    upgrades: Vec<UpgradeDef>,
    providers: Vec<ProviderReg>,
    values: Vec<(TypeId, BoxedValue, String)>,
    backend: Option<Backend>,
    db_url: Option<String>,
    body_limit: usize,
    shutdown_grace: Duration,
    health: bool,
    config: AppConfig,
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    /// App vacía y válida: responde 404 a todo (RF-010).
    pub fn new() -> Self {
        Self {
            modules: Vec::new(),
            globals: Vec::new(),
            upgrades: Vec::new(),
            providers: Vec::new(),
            values: Vec::new(),
            backend: None,
            db_url: None,
            body_limit: DEFAULT_BODY_LIMIT,
            shutdown_grace: Duration::from_secs(30),
            health: true,
            config: AppConfig::default(),
        }
    }

    /// Registra un módulo. Se puede llamar varias veces; el orden no afecta a
    /// la resolución de dependencias (RF-011).
    pub fn module(mut self, module: Module) -> Self {
        self.modules.push(module);
        self
    }

    /// Registra un controller suelto, sin módulo (caso mínimo de §5).
    pub fn controller<C: Controller>(self) -> Self {
        self.module(Module::new(C::NAME).controller::<C>())
    }

    /// Middleware global, en orden de registro: el primero es el más externo (§11.3).
    pub fn middleware<M: Middleware>(mut self, mw: M) -> Self {
        self.globals.push(Arc::new(mw));
        self
    }

    /// Middleware global ya encajado (lo usan `Cors`, `Logger`, la CLI).
    pub fn boxed_middleware(mut self, mw: Arc<dyn Middleware>) -> Self {
        self.globals.push(mw);
        self
    }

    /// Registra un canal de actualización (§10.4, RF-194).
    ///
    /// La ruta usa `/:nombre` (RP-003), se normaliza como cualquier otra
    /// (RF-018) y no participa del 404/405 del fallback. El handshake pasa por
    /// los middlewares globales y después por `SocketHandler::authorize`
    /// (RF-198).
    pub fn socket<H: SocketHandler>(mut self, path: &str, handler: H) -> Self {
        self.upgrades.push(UpgradeDef::new(path, Arc::new(handler)));
        self
    }

    /// URL del motor a usar, elegida por su esquema al arrancar (RF-111).
    ///
    /// No conecta: `build()` es síncrono (RF-118), así que la resolución ocurre en
    /// `connect()`, que `.run()` y `.run_on()` llaman antes de componer.
    pub fn database(mut self, url: impl Into<String>) -> Self {
        self.db_url = Some(url.into());
        self.backend = None;
        self
    }

    /// Adaptador ya conectado, saltando el registro de §12.6 (RF-119).
    pub fn adapter(mut self, adapter: Arc<dyn DbAdapter>) -> Self {
        self.backend = Some(Backend::Adapter(adapter));
        self.db_url = None;
        self
    }

    /// URL de base de datos configurada, sin conectar (para `rdd doctor`).
    pub fn database_url(&self) -> Option<&str> {
        self.db_url.as_deref()
    }

    /// Fuerza el backend en memoria, con prioridad sobre una URL configurada (RF-015).
    pub fn in_memory(mut self) -> Self {
        self.backend = Some(Backend::default());
        self.db_url = None;
        self
    }

    /// Resuelve la URL por el registro y conecta (RF-119). Idempotente: si ya hay
    /// un backend elegido no vuelve a conectar.
    ///
    /// Un esquema sin adaptador instalado falla aquí nombrando el crate que falta,
    /// antes de abrir el socket y nunca en tiempo de petición (RF-111).
    pub async fn connect(mut self) -> crate::Result<Self> {
        if self.backend.is_none()
            && let Some(url) = self.db_url.clone()
        {
            self.backend = Some(Backend::Adapter(crate::db::connect(&url).await?));
        }
        Ok(self)
    }

    /// Desactiva `GET /health` (RF-016).
    pub fn without_health(mut self) -> Self {
        self.health = false;
        self
    }

    /// Activa o desactiva `GET /health` (RF-016).
    pub fn with_health(mut self, enabled: bool) -> Self {
        self.health = enabled;
        self
    }

    /// Límite de cuerpo de petición (RF-017).
    pub fn body_limit(mut self, bytes: usize) -> Self {
        self.body_limit = bytes;
        self
    }

    /// Plazo de drenaje al apagar (RP-009).
    pub fn shutdown_grace(mut self, grace: Duration) -> Self {
        self.shutdown_grace = grace;
        self
    }

    /// Configuración de la app (§15.1).
    pub fn config(mut self, config: AppConfig) -> Self {
        self.config = config;
        self
    }

    /// Compone rutas, middlewares y contenedor sin abrir socket ni hacer I/O
    /// (§4.4, RF-012, RF-118).
    ///
    /// Registra el adaptador activo como valor `Db` del contenedor, que es de donde
    /// `Repo<T>` lo lee (§12.1); por eso un `#[service]` no registra nada por agregado.
    pub fn build(self) -> Result<BuiltApp, ContainerError> {
        let backend = match &self.backend {
            Some(backend) => backend.clone(),
            None if self.db_url.is_some() => {
                return Err(ContainerError::Init {
                    tipo: "DbAdapter".to_owned(),
                    source: anyhow::anyhow!(
                        "hay una URL de base de datos sin conectar y `build()` es síncrono \
                         (RF-118): llama a `App::connect().await` antes, o usa `.run()`/`.run_on()`"
                    ),
                });
            }
            None => Backend::default(),
        };
        let container = self.build_container(backend.adapter())?;

        let mut routes: Vec<RouteDef> = Vec::new();
        for module in &self.modules {
            for controller in &module.controllers {
                routes.extend(controller.defs(&module.middlewares));
            }
        }

        if self.health {
            routes.push(health_route());
        }

        validate_upgrades(&self.upgrades, &routes)?;

        let limits = Limits {
            body_limit_bytes: self.body_limit,
            shutdown_grace: self.shutdown_grace,
            ..Limits::default()
        };

        let http = HttpApp::assemble(
            routes,
            self.upgrades,
            self.globals.clone(),
            Arc::clone(&container),
            limits,
        );

        Ok(BuiltApp {
            http,
            backend,
            container,
        })
    }

    /// Abre el socket, entrega la app a la plataforma por defecto y espera el
    /// apagado (RF-012, RF-013, RP-012).
    ///
    /// Solo existe con `default-platform` activa (RP-012): la instala la fachada
    /// cuando compila con la feature `axum`, así que sin ella llamar a `run` es
    /// un error de compilación y el usuario debe usar `run_on`.
    #[cfg(feature = "default-platform")]
    pub async fn run(self, addr: &str) -> crate::Result<()> {
        let Some(make_platform) = default_platform() else {
            return Err(crate::PlatformError::other(anyhow::anyhow!(NO_DEFAULT_PLATFORM)).into());
        };
        self.run_dyn(make_platform(), addr).await
    }

    /// Sirve con la plataforma indicada (RF-019, RP-013).
    pub async fn run_on<P: Platform>(self, platform: P, addr: &str) -> crate::Result<()> {
        self.run_dyn(Arc::new(platform), addr).await
    }

    /// Núcleo común de los dos anteriores: la plataforma llega encajada para que
    /// `run()` (feature `axum`) y `run_on()` compartan el mismo camino (RP-012).
    ///
    /// Devuelve `Result<()>` y no `PlatformResult<()>` para que el `main` de
    /// §8.4 compile tal cual; `From<PlatformError> for Error` conserva el original.
    async fn run_dyn(self, platform: Arc<dyn Platform>, addr: &str) -> crate::Result<()> {
        // RF-111/RF-119: el motor se resuelve antes de componer y antes de abrir el
        // socket, así que un esquema sin adaptador nunca llega a servir peticiones.
        let app = self.connect().await.map_err(crate::PlatformError::other)?;
        let built = app.build().map_err(crate::PlatformError::Container)?;
        let addr: SocketAddr = addr
            .parse()
            .map_err(|err: std::net::AddrParseError| crate::PlatformError::other(err))?;
        let listener = tokio::net::TcpListener::bind(addr).await.map_err(|err| {
            crate::PlatformError::Bind {
                addr: addr.to_string(),
                source: err,
            }
        })?;
        let local = listener.local_addr().map_err(crate::PlatformError::other)?;

        let shutdown = ShutdownSignal::new();
        log_startup(platform.name(), local, &built);
        spawn_shutdown_trigger(shutdown.clone());

        let name = platform.name();
        let future: PlatformFuture<Result<(), crate::PlatformError>> = platform
            .serve(built.http, listener, shutdown)
            .map_err(|err| {
                tracing::error!(platform = name, error = %err, "la plataforma no arrancó");
                err
            })?;
        Ok(future.await?)
    }

    /// Construye el contenedor validando el grafo (RF-012, RF-023, RF-024, RF-025).
    ///
    /// El adaptador activo entra como valor `Db` (§12.1) y gana sobre un
    /// `.provide(Db(..))` suelto: la fuente de verdad es `.database()`/`.adapter()`/
    /// `.in_memory()`, no un valor registrado a mano.
    fn build_container(
        &self,
        adapter: Arc<dyn DbAdapter>,
    ) -> Result<Arc<crate::di::Container>, ContainerError> {
        let mut builder = ContainerBuilder::new();
        for (id, value, name) in &self.values {
            builder.provide_value(*id, name.clone(), Arc::clone(value));
        }
        builder.provide_value(
            TypeId::of::<Db>(),
            "Db".to_owned(),
            Arc::new(Db(adapter)) as BoxedValue,
        );
        for module in &self.modules {
            for reg in &module.providers {
                builder.provider_ref(reg);
            }
        }
        for reg in &self.providers {
            builder.provider_ref(reg);
        }
        builder.build().map(Arc::new)
    }
}

/// Registro de arranque: dirección, backend, módulos y rutas (RN-042, RP-013).
fn log_startup(platform: &str, addr: SocketAddr, built: &BuiltApp) {
    let routes = built.http.route_listing();
    tracing::info!(
        platform,
        listen = %addr,
        backend = built.backend.label(),
        routes = routes.len(),
        "rdd: servidor listo"
    );
    for route in routes {
        tracing::info!("  montada {route}");
    }
}

/// Convierte `SIGTERM` y `Ctrl+C` en el apagado ordenado (RF-013).
fn spawn_shutdown_trigger(shutdown: ShutdownSignal) {
    tokio::spawn(async move {
        let ctrl_c = async {
            let _ = tokio::signal::ctrl_c().await;
        };
        #[cfg(unix)]
        {
            let term = async {
                match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
                    Ok(mut sig) => sig.recv().await,
                    Err(_) => None,
                }
            };
            tokio::select! {
                _ = ctrl_c => {}
                _ = term => {}
            }
        }
        #[cfg(not(unix))]
        {
            ctrl_c.await;
        }
        tracing::info!("rdd: señal de apagado recibida, drenando");
        shutdown.trigger();
    });
}

/// `GET /health` → 200 `{"status":"ok"}` (RF-016).
fn health_route() -> RouteDef {
    let handler: BoxHandler = Arc::new(|_req| {
        Box::pin(async {
            let resp = http::Response::builder()
                .status(StatusCode::OK)
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(bytes::Bytes::from_static(br#"{"status":"ok"}"#));
            resp.unwrap_or_else(|_| Response::new(bytes::Bytes::from_static(br#"{"status":"ok"}"#)))
        })
    });
    RouteDef::new(Method::GET, "/health", handler)
}

/// Valida que los canales no choquen entre sí ni con una ruta HTTP (RF-194).
///
/// Un choque llegaría a Axum como ruta duplicada y abortaría el arranque; aquí
/// se informa antes de abrir el socket.
fn validate_upgrades(upgrades: &[UpgradeDef], routes: &[RouteDef]) -> Result<(), ContainerError> {
    let mut router = matchit::Router::<()>::new();
    for upgrade in upgrades {
        let path = normalize_path(&upgrade.path);
        if routes
            .iter()
            .any(|route| normalize_path(&route.path) == path)
        {
            return Err(ContainerError::Init {
                tipo: "socket".to_owned(),
                source: anyhow::anyhow!(
                    "el canal `{path}` choca con una ruta HTTP del mismo camino"
                ),
            });
        }
        if let Err(err) = router.insert(to_route_syntax(&path), ()) {
            return Err(ContainerError::Init {
                tipo: "socket".to_owned(),
                source: anyhow::anyhow!("el canal `{path}` es inválido o está duplicado: {err}"),
            });
        }
    }
    Ok(())
}

/// Alias para que la fachada re-exporte el valor por defecto del límite.
pub const fn default_body_limit() -> usize {
    DEFAULT_BODY_LIMIT
}

/// Registro de un valor suelto (RF-022).
impl App {
    /// Registra un valor inyectable por tipo.
    pub fn provide<T: Any + Send + Sync + 'static>(mut self, value: T) -> Self {
        self.values.push((
            TypeId::of::<T>(),
            Arc::new(value) as BoxedValue,
            crate::di::short_name::<T>(),
        ));
        self
    }

    /// Registra el provider de un `#[service]` (RF-021).
    pub fn provider(mut self, reg: ProviderReg) -> Self {
        self.providers.push(reg);
        self
    }
}
