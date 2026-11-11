//! `rdd g module <name>` (§14.1, §14.2, §14.3, RF-001).
//!
//! Genera el árbol de módulo de §5 con el mismo código que produce el módulo de
//! ejemplo de `rdd new`, y lo registra en los dos marcadores de §14.3.

use anyhow::{Result, bail};
use clap::Args;

use crate::files::{self, Insertion};
use crate::module_template::ModuleNames;
use crate::new_project::module_files;

/// Argumentos de `rdd g module`.
#[derive(Args)]
pub struct GModuleArgs {
    /// Nombre del módulo; se normaliza a `snake_case` (RF-142).
    ///
    /// El nombre singular se usa para el agregado de `domain/`; las capas
    /// `application/`, `infrastructure/` y `presentation/` conservan el plural
    /// (RF-005). `tasks` y `task` producen el mismo árbol.
    pub name: String,

    /// Sobrescribe archivos existentes (RF-140).
    #[arg(long)]
    pub force: bool,

    /// Lista los archivos sin escribir nada (RF-141).
    #[arg(long)]
    pub dry_run: bool,
}

/// Ejecuta `rdd g module`.
pub fn execute(args: GModuleArgs) -> Result<()> {
    let root = files::project_root()?;
    let names = ModuleNames::new(&args.name);
    validate(&names)?;

    if !root.join("src/main.rs").is_file() {
        bail!(
            "no se encontró `src/main.rs` en {}; `rdd g module` espera un proyecto \
             creado con `rdd new` (§5)",
            root.display()
        );
    }

    let generated = module_files(&names.dir);
    let insertions = [
        Insertion::new(
            "src/modules/mod.rs",
            "// rdd:mods",
            format!("pub mod {};", names.dir),
        ),
        Insertion::new(
            "src/main.rs",
            "// rdd:modules",
            format!("        .module(modules::{}::module())", names.dir),
        ),
    ];

    files::apply(&root, &generated, &insertions, args.force, args.dry_run)?;
    if args.dry_run {
        return Ok(());
    }

    let dir = &names.dir;
    println!("módulo `{dir}` creado en src/modules/{dir}");
    println!("rutas:");
    println!("  GET    /{dir}");
    println!("  GET    /{dir}/:id");
    println!("  POST   /{dir}");
    println!("  PUT    /{dir}/:id");
    println!("  DELETE /{dir}/:id");
    Ok(())
}

/// Un módulo Rust no puede llamarse como una palabra reservada (RF-142).
fn validate(names: &ModuleNames) -> Result<()> {
    if names.dir.is_empty() {
        bail!("el nombre del módulo no puede quedar vacío después de normalizarlo");
    }
    if KEYWORDS.contains(&names.dir.as_str()) {
        bail!(
            "`{}` es una palabra reservada de Rust; elige otro nombre de módulo",
            names.dir
        );
    }
    Ok(())
}

/// Palabras reservadas y estrictas de Rust, edición 2024.
const KEYWORDS: &[&str] = &[
    "as", "break", "const", "continue", "crate", "else", "enum", "extern", "false", "fn", "for",
    "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return",
    "self", "static", "struct", "super", "trait", "true", "type", "unsafe", "use", "where",
    "while", "async", "await", "dyn", "abstract", "become", "box", "do", "final", "macro",
    "override", "priv", "typeof", "unsized", "virtual", "yield", "try", "gen",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_reserved_words() {
        for name in ["mod", "Type", "self", "async", "gen"] {
            assert!(
                validate(&ModuleNames::new(name)).is_err(),
                "`{name}` no debería aceptarse"
            );
        }
    }

    #[test]
    fn accepts_normal_names() {
        for name in ["tasks", "TaskItems", "users", "order-lines"] {
            assert!(validate(&ModuleNames::new(name)).is_ok(), "`{name}`");
        }
    }

    #[test]
    fn rejects_an_empty_name() {
        assert!(validate(&ModuleNames::new("---")).is_err());
    }
}
