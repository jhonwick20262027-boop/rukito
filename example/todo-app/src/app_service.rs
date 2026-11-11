//! Servicio de la raíz (≈ app.service.ts).
//!
//! Sin reglas de dominio: los casos de uso reales viven en
//! `src/modules/<m>/application/` (§5.1).

pub struct AppService;

impl AppService {
    /// Mensaje que devuelve `AppController`.
    pub fn hello() -> String {
        format!("{} en línea", env!("CARGO_PKG_NAME"))
    }
}
