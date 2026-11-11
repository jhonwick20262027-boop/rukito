//! Puerto de actualización (§10.4, ADR-19).
//!
//! Una conexión WebSocket no cabe en `Request`/`Response` (RP-002, ADR-12). El
//! núcleo decide el handshake y entrega al handler un canal abstracto; la
//! plataforma solo traduce tramas. Así `rdd-core` sigue sin conocer Axum ni
//! `tokio-tungstenite` (RA-011, RA-017).

use std::sync::Arc;

use bytes::Bytes;

use crate::di::Container;
use crate::http::routes::normalize_path;
use crate::http::types::{BoxFuture, Response};

/// Un mensaje del canal, en ambas direcciones (RF-197).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SocketMessage {
    /// Texto UTF-8.
    Text(String),
    /// Datos binarios.
    Binary(Bytes),
    /// Ping con su carga.
    Ping(Bytes),
    /// Pong con su carga.
    Pong(Bytes),
    /// Cierre con código y motivo (`None` cuando el par no lo especifica).
    Close(Option<u16>, String),
}

/// Canal abstracto que la plataforma entrega al handler (RF-195, RF-197).
///
/// La plataforma nunca inventa mensajes: responde los `Ping` con `Pong` y
/// entrega todo lo demás.
pub trait SocketConnection: Send + 'static {
    /// `None` cuando el par cerró o el canal ya está cerrado (RF-197).
    fn recv(&mut self) -> BoxFuture<'_, Option<crate::Result<SocketMessage>>>;

    /// Espera capacidad en la cola de salida; `Err` si el canal ya no sirve
    /// (RF-196).
    fn send(&mut self, msg: SocketMessage) -> BoxFuture<'_, crate::Result<()>>;

    /// Cierra con código y motivo; idempotente (RF-199).
    fn close(self: Box<Self>, code: u16, reason: String) -> BoxFuture<'static, ()>;
}

/// Lo que el handler recibe al aceptarse el handshake (RF-195).
pub struct SocketContext {
    /// Ruta normalizada (RF-018).
    pub path: String,
    /// Parámetros capturados, con la forma de `PathParams` (RP-003).
    pub params: Vec<(String, String)>,
    /// El contenedor de la app (RF-022).
    pub container: Arc<Container>,
    /// Identidad y demás valores que dejaron los globales (RF-083).
    pub extensions: http::Extensions,
}

impl SocketContext {
    /// Valor dejado por un middleware global, igual que `Request::ext` (RF-083).
    pub fn ext<T: Clone + Send + Sync + 'static>(&self) -> Option<T> {
        self.extensions.get::<T>().cloned()
    }

    /// Valor crudo de un parámetro de ruta (RP-003).
    pub fn param(&self, name: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, value)| value.as_str())
    }
}

/// Handler de un canal (RF-194, RF-195).
pub trait SocketHandler: Send + Sync + 'static {
    /// Antes del 101; un `Err` se materializa en Problem Details (RF-198).
    fn authorize(&self, _ctx: &SocketContext) -> crate::Result<()> {
        Ok(())
    }

    /// Vive mientras la conexión vive; al volver en `Ok`, el canal cierra con
    /// 1000 (RF-199).
    fn handle(
        &self,
        ctx: SocketContext,
        conn: Box<dyn SocketConnection>,
    ) -> BoxFuture<'static, crate::Result<()>>;
}

/// Un canal registrado en `HttpApp::upgrades` (RF-194).
#[derive(Clone)]
pub struct UpgradeDef {
    /// Ruta declarada con `/:nombre` y normalizada (RP-003, RF-018).
    pub path: String,
    /// Handler del canal.
    pub handler: Arc<dyn SocketHandler>,
}

impl UpgradeDef {
    /// Normaliza la ruta como cualquier otra (RF-018).
    pub fn new(path: &str, handler: Arc<dyn SocketHandler>) -> Self {
        Self {
            path: normalize_path(path),
            handler,
        }
    }
}

/// Handshake aceptado: el contexto ya lleva la identidad y el handler decidido.
pub struct Handshake {
    /// Contexto que se entrega a `handle`.
    pub context: SocketContext,
    /// Handler del canal.
    pub handler: Arc<dyn SocketHandler>,
}

/// Resultado de `HttpApp::negotiate` (§10.4).
pub enum Negotiation {
    /// La plataforma completa el 101 y llama a `handle`.
    Accept(Handshake),
    /// Rechazo HTTP normal, sin cambio de protocolo (RP-024).
    Reject(Response),
}
