//! Identidad de preview para el token de explorer (§26.9).
//!
//! `--explorer-token` firma un Bearer con el `JWT_SECRET` del servidor para esta
//! identidad, sin registro ni login. El UUID fijo conserva las tareas del
//! explorer entre arranques; `EXPLORER_USER_ID` y `EXPLORER_EMAIL` lo cambian sin
//! tocar código.

use uuid::Uuid;

const DEFAULT_USER_ID: u128 = 0x7d3f_0c4e_5a21_4b8e_9c6d_2f1a_0b3c_4d5e;
const DEFAULT_EMAIL: &str = "explorer@local";

pub fn identity() -> (Uuid, String) {
    let id = super::env_opt("EXPLORER_USER_ID")
        .and_then(|value| value.parse::<Uuid>().ok())
        .unwrap_or_else(|| Uuid::from_u128(DEFAULT_USER_ID));
    let email = super::env_opt("EXPLORER_EMAIL").unwrap_or_else(|| DEFAULT_EMAIL.to_owned());
    (id, email)
}
