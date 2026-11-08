//! # rdd-axum
//!
//! La plataforma HTTP por defecto de `rdd` (§4.1, §4.4). Sirve el [`HttpApp`]
//! del núcleo sobre Axum: enruta con el router de Axum, captura los parámetros
//! `/:nombre` (RP-003), normaliza la barra final (RP-008), aplica el límite de
//! cuerpo (RP-006) y drena al apagar (RP-009).
//!
//! Este crate es el **único** que ve Axum: el núcleo sigue sin conocerlo
//! (RA-011) y ninguna otra plataforma depende de él (RA-012).
//!
//! ```no_run
//! use rdd_axum::AxumPlatform;
//! use rdd_core::app::App;
//!
//! # async fn demo() -> rdd_core::Result<()> {
//! App::new().run_on(AxumPlatform::new(), "127.0.0.1:3000").await
//! # }
//! ```

mod platform;
mod shim;

pub use platform::AxumPlatform;
