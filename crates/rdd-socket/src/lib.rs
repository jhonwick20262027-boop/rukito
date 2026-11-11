//! # rdd-socket
//!
//! Hub de tiempo real de `rdd` (§10.5): salas por nombre, difusión no
//! bloqueante y presencia. Depende solo de `rdd-core` y crates neutrales
//! (RA-017); el transporte lo pone la plataforma y el puerto vive en el núcleo
//! (§10.4).
//!
//! ```no_run
//! use rdd_socket::Hub;
//!
//! # async fn demo() {
//! let hub = Hub::new();
//! let mut sub = hub.subscribe("tasks");
//! hub.broadcast("tasks", "hola").await;
//! assert!(sub.recv().await.is_ok());
//! # }
//! ```
//!
//! El hub es **local al proceso**: sin historial, sin persistencia y sin
//! coordinación entre instancias (§2.2, RF-203). La presencia es el conjunto de
//! suscripciones vivas (RF-201).

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use bytes::Bytes;
use tokio::sync::broadcast;

/// Capacidad por defecto de la cola de cada suscripción (RF-196).
pub const DEFAULT_RING: usize = 64;

/// Lo que viaja por una sala (RF-204).
#[derive(Debug, Clone, PartialEq)]
pub enum Payload {
    /// Texto ya formado.
    Text(String),
    /// Valor JSON; el handler lo serializa sin construir la cadena a mano.
    Json(serde_json::Value),
    /// Datos binarios.
    Binary(Bytes),
}

impl From<String> for Payload {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl From<&str> for Payload {
    fn from(value: &str) -> Self {
        Self::Text(value.to_owned())
    }
}

impl From<serde_json::Value> for Payload {
    fn from(value: serde_json::Value) -> Self {
        Self::Json(value)
    }
}

/// Sucursal atrasada: perdió `0`..`n` mensajes del anillo (RF-203).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lagged(pub u64);

/// Hub de salas del proceso (RF-200, RF-202).
///
/// Se comparte por clon barato; registrado como singleton en el contenedor, un
/// handler lo obtiene con `ctx.container` o por inyección (§10.5).
#[derive(Clone)]
pub struct Hub {
    inner: Arc<Inner>,
}

struct Inner {
    rooms: RwLock<HashMap<String, Arc<Room>>>,
    capacity: usize,
}

impl Default for Hub {
    fn default() -> Self {
        Self::with_capacity(DEFAULT_RING)
    }
}

struct Room {
    tx: broadcast::Sender<Arc<Payload>>,
    members: Mutex<HashSet<String>>,
    next_id: AtomicU64,
}

impl Hub {
    /// Hub vacío con el anillo por defecto.
    pub fn new() -> Self {
        Self::default()
    }

    /// Hub con un anillo de `capacity` mensajes por suscripción.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: Arc::new(Inner {
                rooms: RwLock::new(HashMap::new()),
                capacity: capacity.max(1),
            }),
        }
    }

    /// Suscripción viva mientras se sostenga; soltarla es salir de la sala
    /// (RF-201).
    pub fn subscribe(&self, topic: &str) -> Subscription {
        let room = self.room(topic);
        let id = format!(
            "{topic}#{}",
            room.next_id.fetch_add(1, Ordering::Relaxed) + 1
        );
        if let Ok(mut members) = room.members.lock() {
            members.insert(id.clone());
        }
        let rx = room.tx.subscribe();
        Subscription {
            id,
            topic: topic.to_owned(),
            rx,
            room,
            inner: Arc::clone(&self.inner),
        }
    }

    /// Difunde a la sala y devuelve cuántas suscripciones recibieron (RF-200).
    ///
    /// No serializa la entrega: cada suscripción tiene su propio anillo
    /// (RF-202).
    pub async fn broadcast(&self, topic: &str, msg: impl Into<Payload>) -> usize {
        let room = match self.inner.rooms.read() {
            Ok(rooms) => rooms.get(topic).cloned(),
            Err(_) => None,
        };
        match room {
            Some(room) => room.tx.send(Arc::new(msg.into())).unwrap_or(0),
            None => 0,
        }
    }

    /// Presencia: miembros vivos de la sala, ordenados (RF-201).
    pub fn members(&self, topic: &str) -> Vec<String> {
        let room = match self.inner.rooms.read() {
            Ok(rooms) => rooms.get(topic).cloned(),
            Err(_) => None,
        };
        let mut members: Vec<String> = room
            .and_then(|room| {
                room.members
                    .lock()
                    .ok()
                    .map(|m| m.iter().cloned().collect())
            })
            .unwrap_or_default();
        members.sort();
        members
    }

    fn room(&self, topic: &str) -> Arc<Room> {
        if let Ok(rooms) = self.inner.rooms.read()
            && let Some(room) = rooms.get(topic)
        {
            return Arc::clone(room);
        }
        let mut rooms = self
            .inner
            .rooms
            .write()
            .expect("el mapa de salas no debe envenenarse");
        rooms
            .entry(topic.to_owned())
            .or_insert_with(|| {
                Arc::new(Room {
                    tx: broadcast::channel(self.inner.capacity).0,
                    members: Mutex::new(HashSet::new()),
                    next_id: AtomicU64::new(0),
                })
            })
            .clone()
    }
}

/// Una suscripción viva a una sala (RF-200, RF-201).
pub struct Subscription {
    id: String,
    topic: String,
    rx: broadcast::Receiver<Arc<Payload>>,
    room: Arc<Room>,
    inner: Arc<Inner>,
}

impl Subscription {
    /// Siguiente mensaje de la sala; `Lagged` si el suscriptor se atrasó
    /// (RF-203).
    pub async fn recv(&mut self) -> Result<Payload, Lagged> {
        match self.rx.recv().await {
            Ok(payload) => Ok((*payload).clone()),
            Err(broadcast::error::RecvError::Lagged(skipped)) => Err(Lagged(skipped)),
            Err(broadcast::error::RecvError::Closed) => Err(Lagged(0)),
        }
    }

    /// Sala a la que pertenece.
    pub fn topic(&self) -> &str {
        &self.topic
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        let empty = self
            .room
            .members
            .lock()
            .map(|mut members| {
                members.remove(&self.id);
                members.is_empty()
            })
            .unwrap_or(false);
        if !empty {
            return;
        }
        if let Ok(mut rooms) = self.inner.rooms.write()
            && let Some(room) = rooms.get(&self.topic)
            && Arc::ptr_eq(room, &self.room)
        {
            rooms.remove(&self.topic);
        }
    }
}

impl std::fmt::Debug for Subscription {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Subscription")
            .field("id", &self.id)
            .field("topic", &self.topic)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn dos_suscripciones_reciben_el_mismo_evento() {
        let hub = Hub::new();
        let mut first = hub.subscribe("tasks");
        let mut second = hub.subscribe("tasks");

        assert_eq!(hub.broadcast("tasks", "creada").await, 2);
        assert_eq!(
            first.recv().await.unwrap(),
            Payload::Text("creada".to_owned())
        );
        assert_eq!(
            second.recv().await.unwrap(),
            Payload::Text("creada".to_owned())
        );
    }

    #[tokio::test]
    async fn una_sala_sin_suscripciones_no_retiene_historial() {
        let hub = Hub::new();
        assert_eq!(hub.broadcast("tasks", "antes").await, 0);

        let mut late = hub.subscribe("tasks");
        let quiet = tokio::time::timeout(std::time::Duration::from_millis(50), late.recv()).await;
        assert!(quiet.is_err(), "la suscripción nueva no debe ver el pasado");
    }

    #[tokio::test]
    async fn presencia_refleja_las_suscripciones_vivas() {
        let hub = Hub::new();
        let first = hub.subscribe("tasks");
        let second = hub.subscribe("tasks");
        assert_eq!(hub.members("tasks").len(), 2);

        drop(first);
        assert_eq!(hub.members("tasks"), vec![second.id.clone()]);

        drop(second);
        assert!(hub.members("tasks").is_empty());
    }

    #[tokio::test]
    async fn un_suscriptor_atrasado_recibe_lagged() {
        let hub = Hub::new();
        let mut sub = hub.subscribe("tasks");
        for n in 0..(DEFAULT_RING + 3) {
            hub.broadcast("tasks", format!("m{n}")).await;
        }
        let first = sub.recv().await;
        // El anillo conserva los últimos `DEFAULT_RING`; lo anterior se reporta.
        assert_eq!(first, Err(Lagged(3)));
        assert_eq!(sub.recv().await, Ok(Payload::Text("m3".to_owned())));
    }

    #[tokio::test]
    async fn json_y_texto_conservan_su_forma() {
        let hub = Hub::new();
        let mut sub = hub.subscribe("tasks");
        hub.broadcast("tasks", serde_json::json!({"id": 1})).await;
        assert_eq!(
            sub.recv().await.unwrap(),
            Payload::Json(serde_json::json!({"id": 1}))
        );
    }
}
