//! Transportes del servidor MCP. En 0.7 solo existe stdio (§10.7, ADR-21).

mod stdio;

pub use stdio::{serve_io, serve_stdio};
