//! Interfaz de plataforma (§4.4) y señal de apagado (RP-009).

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use tokio::net::TcpListener;

use crate::error::{PlatformError, PlatformResult};
use crate::http::routes::HttpApp;

/// Caja de futuro de una plataforma: `'static` porque el servidor vive más que
/// la llamada a `serve`.
pub type PlatformFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;

/// Señal de apagado ordenado. El núcleo la dispara al recibir `SIGTERM` o `Ctrl+C`
/// (RF-013); la plataforma deja de aceptar conexiones y drena (RP-009).
#[derive(Clone)]
pub struct ShutdownSignal {
    notify: Arc<ShutdownState>,
}

/// Estado compartido de la señal: el `Notify` y el flag de disparo.
#[derive(Default)]
struct ShutdownState {
    notify: tokio::sync::Notify,
    triggered: std::sync::atomic::AtomicBool,
}

impl Default for ShutdownSignal {
    fn default() -> Self {
        Self::new()
    }
}

impl ShutdownSignal {
    /// Señal nueva, sin activar.
    pub fn new() -> Self {
        Self {
            notify: Arc::new(ShutdownState::default()),
        }
    }

    /// Activa el apagado. Idempotente.
    pub fn trigger(&self) {
        self.notify
            .triggered
            .store(true, std::sync::atomic::Ordering::SeqCst);
        self.notify.notify.notify_waiters();
    }

    /// Espera a que se active. Devuelve inmediatamente si ya se activó antes.
    pub async fn wait(&self) {
        let notified = self.notify.notify.notified();
        tokio::pin!(notified);
        while !self.is_triggered() {
            notified.as_mut().await;
        }
    }

    /// Cierto si el apagado ya fue activado.
    pub fn is_triggered(&self) -> bool {
        self.notify
            .triggered
            .load(std::sync::atomic::Ordering::SeqCst)
    }
}

/// Plataforma HTTP intercambiable (§4.4, RP-001).
///
/// Solo sabe servir un `HttpApp` ya compuesto: enrutar, leer cuerpos con límite,
/// normalizar la barra final y drenar al apagar. No contiene lógica de aplicación.
///
/// `serve` recibe `Arc<Self>` —no `self` a secas como esboza §4.4— para que la
/// plataforma por defecto pueda guardarse como `Arc<dyn Platform>` y `App::run`
/// y `App::run_on` compartan un único camino. El uso declarado en el spec no
/// cambia: `run_on(HyperPlatform::new(), addr)`.
pub trait Platform: Send + Sync + 'static {
    /// Nombre de la plataforma, registrado al arrancar (RP-013, RN-042).
    fn name(&self) -> &'static str;

    /// Sirve `app` sobre el `listener` hasta que `shutdown` se active.
    fn serve(
        self: Arc<Self>,
        app: HttpApp,
        listener: TcpListener,
        shutdown: ShutdownSignal,
    ) -> PlatformResult<PlatformFuture<Result<(), PlatformError>>>;
}

/// Plataforma por defecto para `App::run` (RP-012).
///
/// La fachada `rdd` la instala al inicializarse; si el usuario compila sin la
/// feature `axum`, la fachada define el slot con un error de compilación claro.
pub type DefaultPlatformFn = fn() -> Arc<dyn Platform>;

/// Slot de la plataforma por defecto. Escrito una vez por la fachada.
pub struct DefaultPlatformSlot {
    inner: std::sync::OnceLock<DefaultPlatformFn>,
}

impl DefaultPlatformSlot {
    const fn new() -> Self {
        Self {
            inner: std::sync::OnceLock::new(),
        }
    }

    /// Instala la plataforma por defecto. La primera escritura gana.
    pub fn set(&self, f: DefaultPlatformFn) -> bool {
        self.inner.set(f).is_ok()
    }

    /// Plataforma por defecto instalada.
    pub fn get(&self) -> Option<DefaultPlatformFn> {
        self.inner.get().copied()
    }
}

impl Default for DefaultPlatformSlot {
    fn default() -> Self {
        Self::new()
    }
}

/// Slot global usado por la fachada para registrar Axum como plataforma por defecto.
pub static DEFAULT_PLATFORM: DefaultPlatformSlot = DefaultPlatformSlot::new();

/// Registra la plataforma por defecto del binario (RF-012, RP-012).
///
/// Lo llama la fachada `rdd` en su inicialización; no es API de usuario.
pub fn set_default_platform(f: DefaultPlatformFn) -> bool {
    DEFAULT_PLATFORM.set(f)
}

/// Plataforma por defecto instalada.
pub fn default_platform() -> Option<DefaultPlatformFn> {
    DEFAULT_PLATFORM.get()
}
