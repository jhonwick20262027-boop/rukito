//! CLI `rdd` (§14): `new`, `g module` y `run`.
//!
//! La CLI es un crate aparte (`rdd-cli`) con binario `rdd`; no depende de
//! `rdd-core` ni de ninguna plataforma, solo genera proyectos y delega en cargo.

use clap::{Parser, Subcommand};

mod files;
mod g_module;
mod migrations;
mod module_template;
mod naming;
mod new_project;
mod run_app;
mod templates;

#[derive(Parser)]
#[command(name = "rdd", version, about = "CLI del framework rdd")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Crea un proyecto nuevo con el árbol funcional mínimo de §5.
    New(new_project::NewArgs),
    /// Generadores de código (`rdd g …`).
    #[command(subcommand)]
    G(Generate),
    /// Ejecuta la app del proyecto actual.
    Run(run_app::RunArgs),
    /// Aplica las migraciones pendientes antes del despliegue.
    Migrate(migrations::MigrateArgs),
}

/// Subcomandos de `rdd g` (§14.1).
#[derive(Subcommand)]
enum Generate {
    /// Genera un módulo con la convención NestJS de §5 y lo registra (§14.3).
    Module(g_module::GModuleArgs),
}

fn main() -> anyhow::Result<()> {
    match Cli::parse().command {
        Command::New(args) => new_project::execute(args),
        Command::G(Generate::Module(args)) => g_module::execute(args),
        Command::Run(args) => run_app::execute(args),
        Command::Migrate(args) => migrations::execute(args),
    }
}
