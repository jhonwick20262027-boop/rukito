//! Rol de la identidad autenticada (§26.3).
//!
//! Es un objeto de valor, no el agregado: por eso sí deriva `Deserialize`, que
//! RF-031 le prohíbe a `User`. Lo usa el `Claims` del JWT en infraestructura.

use serde::{Deserialize, Serialize};

/// Jerarquía mínima del demo: `admin` puede todo lo que puede `user`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Admin,
}

impl Role {
    /// Texto que se guarda en la columna `role` y viaja en el token.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Admin => "admin",
        }
    }

    /// `None` ante un texto desconocido: una fila corrupta no puede convertirse
    /// en `Admin` por accidente.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "user" => Some(Self::User),
            "admin" => Some(Self::Admin),
            _ => None,
        }
    }

    /// Si este rol alcanza lo que exige la ruta (`RequireRole`, §26.3).
    pub const fn satisfies(self, required: Self) -> bool {
        match (self, required) {
            (Self::Admin, _) => true,
            (Self::User, Self::User) => true,
            (Self::User, Self::Admin) => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_texto_ida_y_vuelta() {
        for role in [Role::User, Role::Admin] {
            assert_eq!(Role::parse(role.as_str()), Some(role));
        }
        assert_eq!(Role::parse("root"), None);
        assert_eq!(Role::parse(""), None);
    }

    #[test]
    fn admin_alcanza_todo_y_user_solo_lo_suyo() {
        assert!(Role::Admin.satisfies(Role::User));
        assert!(Role::Admin.satisfies(Role::Admin));
        assert!(Role::User.satisfies(Role::User));
        assert!(!Role::User.satisfies(Role::Admin));
    }

    #[test]
    fn se_serializa_en_minúsculas() {
        assert_eq!(serde_json::to_string(&Role::Admin).unwrap(), "\"admin\"");
        assert_eq!(
            serde_json::from_str::<Role>("\"user\"").unwrap(),
            Role::User
        );
    }
}
