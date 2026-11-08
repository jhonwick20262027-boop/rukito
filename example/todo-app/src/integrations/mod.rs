//! Clientes de sistemas externos (§5.1). Un crate externo por integración (`rdd add`, §14.1) registrado como provider; aquí no se decide negocio.
//!
//! Cada subcarpeta documenta qué va ahí y qué API de rdd lo cubre (RF-004).

pub mod aws;
pub mod email;
pub mod payment_gateways;
pub mod telegram;
