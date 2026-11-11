//! Identidad autenticada que viaja en las extensiones de la petición (RF-083).
//!
//! La pone `JwtMiddleware` y la leen `RequireAuth`, `RequireRole` y el handler
//! con `Ext<Session>`. Es lo mínimo que hace falta para autorizar: ni el hash de
//! la contraseña ni el token entero, que no deben salir de infraestructura.

use uuid::Uuid;

use crate::modules::users::domain::Role;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
    pub id: Uuid,
    pub email: String,
    pub role: Role,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RF-083: `Ext<T>` y `Request::ext` exigen `Clone + Send + Sync + 'static`.
    #[test]
    fn cumple_lo_que_exige_ext() {
        fn assert_ext<T: Clone + Send + Sync + 'static>() {}
        assert_ext::<Session>();
    }
}
