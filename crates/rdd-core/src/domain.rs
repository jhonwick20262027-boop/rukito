//! Building blocks DDD (§7).

/// Evento de dominio. Reservado: el núcleo no los consume en 0.x (§7.1).
pub trait DomainEvent: Send + Sync + std::fmt::Debug {}

/// Entidad raíz que protege invariantes (§7.1).
///
/// Se modifica solo mediante sus métodos; la macro `#[aggregate]` genera esta
/// implementación (RF-031).
pub trait Aggregate: Send + Sync + 'static {
    /// Tipo del identificador. `Repo<T>` exige además `Into<Value>` (§12.1).
    type Id: Clone + Send + Sync + 'static;

    /// Identificador del agregado.
    fn id(&self) -> &Self::Id;

    /// Reservado para eventos de dominio. NO implementar comportamiento en 0.x.
    fn take_events(&mut self) -> Vec<Box<dyn DomainEvent>> {
        Vec::new()
    }
}
