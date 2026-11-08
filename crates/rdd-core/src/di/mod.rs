//! Inyección de dependencias por contenedor (§6.3).
//!
//! Rust no tiene reflexión: los `#[service]` generan constructores tipados y el
//! contenedor valida el grafo **al arrancar** (P7, ADR-06).

mod container;

use std::sync::{Arc, OnceLock};

use crate::error::{Error, Result};

pub use crate::error::ContainerError;
pub use container::{BoxedValue, Container, ContainerBuilder, ProviderReg, short_name};

/// Valor construible desde el contenedor (RF-022).
///
/// Lo implementan los `#[service]` generados, `Repo<T>` y `Config<T>`. La
/// convención es parte del contrato: **un valor no registrado se reporta con
/// [`Error::not_found`]**, que [`Container::resolve`] reescribe como el mensaje
/// completo de RF-024 — con solicitante y campo— al resolver dentro de un
/// constructor. Cualquier otro error se propaga tal cual.
pub trait FromContainer: Send + Sized {
    /// Nombre corto del tipo, para el mensaje de RF-024.
    const NAME: &'static str;

    /// Lee o construye el valor. Debe devolver [`Error::not_found`] si falta.
    fn from_container(container: &Container) -> Result<Self, Error>;
}

/// Error de RF-022: el tipo no es inyectable. Lo consumen las macros al construir
/// el mensaje RDD0201 y el contenedor al fallar la resolución.
pub fn not_injectable<T: ?Sized>(hint: &str) -> crate::error::Error {
    crate::Error::internal(anyhow::anyhow!(
        "el campo de tipo `{}` no puede inyectarse: {hint}\n  \
         registra el valor con `.provide::<T>(...)` o convierte `T` en un `#[service]` (RF-022)",
        std::any::type_name::<T>()
    ))
}

/// Contenedor compartido usado cuando una petición llega sin uno inyectado
/// (`TestApp` suelto, benches, `dispatch` manual).
pub fn default_container() -> &'static Arc<Container> {
    static DEFAULT: OnceLock<Arc<Container>> = OnceLock::new();
    DEFAULT.get_or_init(|| Arc::new(Container::empty()))
}

/// El contenedor global como `Option`, para montar rutas sueltas.
pub fn try_default_container() -> Option<Arc<Container>> {
    Some(Arc::clone(default_container()))
}
