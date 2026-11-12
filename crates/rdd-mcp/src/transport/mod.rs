//! Transportes del servidor MCP: stdio (§10.7) y HTTP (§10.8).

mod stdio;

pub use stdio::{MAX_LINE_BYTES, serve_io, serve_io_with, serve_stdio, serve_stdio_with};
