//! Guardas de autorización: cortan la petición antes del handler.
//!
//! API de rdd: middlewares que devuelven `Error::Unauthorized` o `Error::Forbidden` (§11, RF-082).
