//! `rdd new <name>` (§14.1, §5, RF-002).
//!
//! Genera el árbol completo, se niega a sobrescribir sin `--force` (RF-140) y
//! lista sin escribir con `--dry-run` (RF-141).

use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use clap::Args;

use crate::files::{self, GeneratedFile};
use crate::module_template::{self, ModuleNames};
use crate::naming::{kebab_case, snake_case};
use crate::templates::{self, ProjectContext};

/// Argumentos de `rdd new`.
#[derive(Args)]
pub struct NewArgs {
    /// Nombre del proyecto; se normaliza a `kebab-case` (RF-142).
    pub name: String,

    /// No genera el módulo de ejemplo `tasks`.
    #[arg(long)]
    pub no_example: bool,

    /// Sobrescribe archivos existentes (RF-140).
    #[arg(long)]
    pub force: bool,

    /// Lista los archivos sin escribir nada (RF-141).
    #[arg(long)]
    pub dry_run: bool,

    /// Ruta local al crate `rdd` (solo para desarrollo del framework).
    #[arg(long, hide = true)]
    pub rdd_path: Option<PathBuf>,
}

/// Ejecuta `rdd new`.
pub fn execute(args: NewArgs) -> Result<()> {
    let package = kebab_case(&args.name);
    let database = snake_case(&args.name);
    if package.is_empty() {
        bail!("el nombre del proyecto no puede quedar vacío después de normalizarlo");
    }
    if !package.starts_with(|c: char| c.is_ascii_alphabetic()) {
        bail!("`{package}` no es un nombre de paquete Cargo válido: debe empezar por una letra");
    }

    let dependency = rdd_dependency(args.rdd_path.as_deref());
    let ctx = ProjectContext {
        package: &package,
        database: &database,
        rdd_dependency: &dependency,
    };

    let with_example = !args.no_example;
    let root = PathBuf::from(&package);
    let mut generated = templates::project_files(&ctx, with_example);
    if with_example {
        // §14.1: el módulo de ejemplo es siempre `tasks`, no el nombre del
        // proyecto, y sale del mismo generador que `rdd g module`.
        generated.extend(module_files("tasks"));
    }

    files::apply(&root, &generated, &[], args.force, args.dry_run)?;
    if args.dry_run {
        return Ok(());
    }

    println!("proyecto `{package}` creado en ./{package}");
    println!("siguiente paso:");
    println!("  cd {package}");
    println!("  rdd run --memory");
    Ok(())
}

/// Los archivos de un módulo, ya colocados bajo `src/modules/<dir>/`.
pub fn module_files(name: &str) -> Vec<GeneratedFile> {
    let names = ModuleNames::new(name);
    let base = Path::new("src/modules").join(&names.dir);
    module_template::files(&names)
        .into_iter()
        .map(|file| GeneratedFile::new(base.join(file.path), file.contents))
        .collect()
}

/// Dependencia TOML de `rdd`: versión publicada o ruta local para desarrollo.
fn rdd_dependency(local: Option<&Path>) -> String {
    match local {
        Some(path) => {
            let resolved = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
            let display = resolved.display().to_string();
            // `canonicalize` en Windows devuelve `\\?\C:\…`; cargo acepta la ruta
            // normal, así que se quita el prefijo verbatim.
            let display = display.strip_prefix(r"\\?\").unwrap_or(&display);
            format!("{{ path = \"{}\" }}", display.replace('\\', "/"))
        }
        None => "\"0.1\"".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};

    fn context() -> ProjectContext<'static> {
        ProjectContext {
            package: "todo-app",
            database: "todo_app",
            rdd_dependency: "\"0.1\"",
        }
    }

    fn all_files(with_example: bool) -> Vec<GeneratedFile> {
        let ctx = context();
        let mut generated = templates::project_files(&ctx, with_example);
        if with_example {
            generated.extend(module_files("tasks"));
        }
        generated
    }

    fn paths(with_example: bool) -> Vec<String> {
        all_files(with_example)
            .iter()
            .map(|file| file.path.display().to_string().replace('\\', "/"))
            .collect()
    }

    #[test]
    fn generates_the_tree_of_section_five() {
        let paths = paths(true);
        for expected in [
            "Cargo.toml",
            "rdd.toml",
            "rust-toolchain.toml",
            "rustfmt.toml",
            "clippy.toml",
            "Dockerfile",
            ".env",
            ".env.example",
            ".gitignore",
            "docs/.gitkeep",
            "prisma/schema.prisma",
            "prisma/migrations/.gitkeep",
            "tests/acceptance.rs",
            "tests/e2e/.gitkeep",
            "tests/fixtures/.gitkeep",
            "src/main.rs",
            "src/app_module.rs",
            "src/app_controller.rs",
            "src/app_service.rs",
            "src/config/app_config.rs",
            "src/config/database_config.rs",
            "src/config/jwt_config.rs",
            "src/config/redis_config.rs",
            "src/common/decorators/mod.rs",
            "src/common/filters/mod.rs",
            "src/common/guards/mod.rs",
            "src/common/interceptors/mod.rs",
            "src/common/middleware/mod.rs",
            "src/common/pipes/mod.rs",
            "src/common/constants/mod.rs",
            "src/common/enums/mod.rs",
            "src/common/interfaces/mod.rs",
            "src/common/exceptions/mod.rs",
            "src/common/utils/mod.rs",
            "src/modules/mod.rs",
            "src/modules/tasks/mod.rs",
            "src/modules/tasks/controllers/tasks_controller.rs",
            "src/modules/tasks/dto/create_task_dto.rs",
            "src/modules/tasks/entities/task_entity.rs",
            "src/modules/tasks/interfaces/task_interface.rs",
            "src/modules/tasks/mappers/task_mapper.rs",
            "src/modules/tasks/tests/tasks_service_test.rs",
            "src/infrastructure/database/mod.rs",
            "src/infrastructure/external_services/mod.rs",
            "src/integrations/aws/mod.rs",
            "src/integrations/payment_gateways/mod.rs",
            "src/shared/types/mod.rs",
            "src/shared/contracts/mod.rs",
            "src/generated/mod.rs",
        ] {
            assert!(paths.contains(&expected.to_owned()), "falta {expected}");
        }
    }

    #[test]
    fn no_folder_is_left_empty() {
        let generated = all_files(true);
        for file in &generated {
            if file.path.extension().is_some_and(|ext| ext == "rs") {
                assert!(
                    !file.contents.trim().is_empty(),
                    "{} está vacío (RF-003)",
                    file.path.display()
                );
            }
        }

        let declared: BTreeSet<&Path> = generated.iter().map(|file| file.path.as_path()).collect();

        // RF-003: las carpetas que no son módulos Rust llevan `.gitkeep`.
        for keep in [
            "docs/.gitkeep",
            "prisma/migrations/.gitkeep",
            "tests/e2e/.gitkeep",
            "tests/fixtures/.gitkeep",
        ] {
            assert!(declared.contains(Path::new(keep)), "falta {keep} (RF-003)");
        }

        // RF-003: toda carpeta Rust bajo `src/` lleva `mod.rs`; `src/` lo indexa
        // main.rs y `prisma/` ya tiene schema.prisma, así que no queda vacía.
        let folders: BTreeSet<&Path> = generated
            .iter()
            .filter_map(|file| file.path.parent())
            .collect();
        for folder in folders {
            if !folder.starts_with("src") || folder == Path::new("src") {
                continue;
            }
            assert!(
                declared.contains(folder.join("mod.rs").as_path()),
                "{} no tiene mod.rs (RF-003)",
                folder.display()
            );
        }
    }

    #[test]
    fn every_rust_file_is_declared_by_its_parent() {
        let generated = all_files(true);
        let by_path: BTreeMap<&Path, &str> = generated
            .iter()
            .map(|file| (file.path.as_path(), file.contents.as_str()))
            .collect();

        for file in &generated {
            let path = file.path.as_path();
            let name = path.file_name().and_then(|name| name.to_str()).unwrap();
            // main.rs y acceptance.rs son raíces de crate; mod.rs es el índice.
            if !path.extension().is_some_and(|ext| ext == "rs")
                || name == "mod.rs"
                || path == Path::new("src/main.rs")
                || path == Path::new("tests/acceptance.rs")
            {
                continue;
            }
            let stem = path.file_stem().and_then(|stem| stem.to_str()).unwrap();
            let parent = path.parent().unwrap();
            let index = if parent == Path::new("src") {
                Path::new("src/main.rs")
            } else {
                &parent.join("mod.rs")
            };
            let contents = by_path
                .get(index)
                .unwrap_or_else(|| panic!("falta el índice {}", index.display()));
            assert!(
                contents.contains(&format!("mod {stem};")),
                "{} no está declarado en {}",
                path.display(),
                index.display()
            );
        }
    }

    #[test]
    fn no_example_skips_the_module() {
        let paths = paths(false);
        assert!(paths.contains(&"src/modules/mod.rs".to_owned()));
        assert!(
            !paths
                .iter()
                .any(|path| path.starts_with("src/modules/tasks")),
            "`--no-example` no debe generar el módulo"
        );
    }

    #[test]
    fn main_keeps_the_markers_in_both_files() {
        let generated = all_files(true);
        let main = generated
            .iter()
            .find(|file| file.path == Path::new("src/main.rs"))
            .unwrap();
        assert!(main.contents.contains("// rdd:modules"));
        assert!(
            main.contents
                .contains("        .module(modules::tasks::module())\n")
        );
        let modules = generated
            .iter()
            .find(|file| file.path == Path::new("src/modules/mod.rs"))
            .unwrap();
        assert!(modules.contents.contains("// rdd:mods\npub mod tasks;\n"));
    }
}
