//! `rdd run [--memory]` (§14.1).
//!
//! Ejecuta `cargo run` en el proyecto actual. `--memory` deja `RDD_MEMORY=1`,
//! que el `main.rs` del scaffold traduce a `App::in_memory()` (RF-015).

use std::process::Command;

use anyhow::{Context, Result, bail};
use clap::Args;

use crate::files;

/// Argumentos de `rdd run`.
#[derive(Args)]
pub struct RunArgs {
    /// Usa el backend en memoria (sin base de datos).
    #[arg(long)]
    pub memory: bool,

    /// Argumentos que se pasan a la aplicación, después de `--`.
    #[arg(last = true)]
    pub passthrough: Vec<String>,
}

/// Ejecuta `rdd run`.
pub fn execute(args: RunArgs) -> Result<()> {
    let root = files::project_root()?;

    let mut command = Command::new("cargo");
    command.arg("run").current_dir(&root);
    if !args.passthrough.is_empty() {
        command.arg("--").args(&args.passthrough);
    }
    if args.memory {
        command.env("RDD_MEMORY", "1");
    }

    let status = command
        .status()
        .context("no se pudo ejecutar `cargo run`; ¿está cargo en el PATH?")?;
    if !status.success() {
        bail!("`cargo run` terminó con {status}");
    }
    Ok(())
}
