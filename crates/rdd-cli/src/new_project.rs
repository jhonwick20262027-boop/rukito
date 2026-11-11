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
use crate::templates::{self, Database, ProjectContext};

/// Argumentos de `rdd new`.
#[derive(Args)]
pub struct NewArgs {
    /// Nombre del proyecto; se normaliza a `kebab-case` (RF-142).
    pub name: String,

    /// Motor predeterminado del proyecto (RF-189).
    #[arg(long, value_enum, default_value_t = Database::Sqlite)]
    pub database: Database,

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

    let dependency = rdd_dependency(args.rdd_path.as_deref(), args.database);
    let ctx = ProjectContext {
        package: &package,
        database: &database,
        rdd_dependency: &dependency,
        engine: args.database,
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
    if args.database == Database::Postgres {
        println!("edita .env: la URL lleva `CAMBIAR` por contraseña y apunta a localhost:5432");
    }
    println!("siguiente paso:");
    println!("  cd {package}");
    println!("  rdd migrate");
    println!("  rdd run");
    println!("usa `rdd run --memory` para omitir la base de datos");
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

/// Dependencia TOML de `rdd`: versión publicada o ruta local para desarrollo,
/// más la feature del motor cuando no viene ya en `default` (RF-189).
fn rdd_dependency(local: Option<&Path>, engine: Database) -> String {
    let source = match local {
        Some(path) => {
            let resolved = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
            let display = resolved.display().to_string();
            // `canonicalize` en Windows devuelve `\\?\C:\…`; cargo acepta la ruta
            // normal, así que se quita el prefijo verbatim.
            let display = display.strip_prefix(r"\\?\").unwrap_or(&display);
            format!("path = \"{}\"", display.replace('\\', "/"))
        }
        None => "version = \"0.1\"".to_owned(),
    };
    // `sqlite` está en `default` de la fachada (§12.6): nombrarlo no añade nada.
    let features = if engine == Database::Sqlite {
        String::new()
    } else {
        format!(", features = [\"{}\"]", engine.feature())
    };
    if local.is_none() && features.is_empty() {
        "\"0.1\"".to_owned()
    } else {
        format!("{{ {source}{features} }}")
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
            engine: Database::Sqlite,
        }
    }

    fn files_for(ctx: &ProjectContext<'_>, with_example: bool) -> Vec<GeneratedFile> {
        let mut generated = templates::project_files(ctx, with_example);
        if with_example {
            generated.extend(module_files("tasks"));
        }
        generated
    }

    fn all_files(with_example: bool) -> Vec<GeneratedFile> {
        files_for(&context(), with_example)
    }

    fn contents<'a>(files: &'a [GeneratedFile], path: &str) -> &'a str {
        files
            .iter()
            .find(|file| file.path == Path::new(path))
            .unwrap_or_else(|| panic!("falta {path}"))
            .contents
            .as_str()
    }

    fn paths(with_example: bool) -> Vec<String> {
        all_files(with_example)
            .iter()
            .map(|file| file.path.display().to_string().replace('\\', "/"))
            .collect()
    }

    #[test]
    fn generates_a_minimal_functional_project_tree() {
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
            "migrations/0001_tasks.sql",
            "prisma/schema.prisma",
            "tests/acceptance.rs",
            "src/main.rs",
            "src/app_module.rs",
            "src/app_controller.rs",
            "src/app_service.rs",
            "src/config/app_config.rs",
            "src/config/database_config.rs",
            "src/modules/mod.rs",
            "src/modules/tasks/mod.rs",
            "src/modules/tasks/domain/mod.rs",
            "src/modules/tasks/domain/task.rs",
            "src/modules/tasks/domain/tasks_repository.rs",
            "src/modules/tasks/application/mod.rs",
            "src/modules/tasks/application/tasks_use_cases.rs",
            "src/modules/tasks/infrastructure/mod.rs",
            "src/modules/tasks/infrastructure/tasks_repository.rs",
            "src/modules/tasks/presentation/mod.rs",
            "src/modules/tasks/presentation/dto.rs",
            "src/modules/tasks/presentation/tasks_controller.rs",
            "src/generated/mod.rs",
        ] {
            assert!(paths.contains(&expected.to_owned()), "falta {expected}");
        }

        for placeholder in [
            "docs/.gitkeep",
            "prisma/migrations/.gitkeep",
            "tests/e2e/.gitkeep",
            "tests/fixtures/.gitkeep",
            "src/config/jwt_config.rs",
            "src/config/redis_config.rs",
            "src/common/mod.rs",
            "src/infrastructure/mod.rs",
            "src/integrations/mod.rs",
            "src/shared/mod.rs",
        ] {
            assert!(
                !paths.contains(&placeholder.to_owned()),
                "no debe generar el placeholder {placeholder}"
            );
        }
    }

    /// RF-004: ningún `.rs` vacío ni solo cabecera. Las dos excepciones que
    /// nombra el spec se reconocen por su contenido, no por su nombre: un snippet
    /// documental (bloque cercado) o el marcador donde inserta `rdd g module`.
    fn assert_no_blank_rust(with_example: bool) {
        for file in all_files(with_example) {
            if !file.path.extension().is_some_and(|ext| ext == "rs") {
                continue;
            }
            let path = file.path.display();
            assert!(
                !file.contents.trim().is_empty(),
                "{path} está vacío (RF-004)"
            );

            let has_code = file
                .contents
                .lines()
                .map(str::trim)
                .any(|line| !line.is_empty() && !line.starts_with("//"));
            let has_snippet = file.contents.contains("```");
            let has_marker = file.contents.contains("// rdd:mods");
            assert!(
                has_code || has_snippet || has_marker,
                "{path} es solo cabecera (RF-004)"
            );
        }
    }

    #[test]
    fn no_rust_file_is_blank_or_header_only() {
        assert_no_blank_rust(true);
        assert_no_blank_rust(false);
    }

    /// RF-192: `rdd-sqlite` decodifica por el nombre de tipo declarado, así que
    /// `TEXT`/`INTEGER` —que SQLite aceptaría— harían llegar `Value::Text` y
    /// `Value::I64` a un `from_row` que espera `Uuid` y `Bool`.
    #[test]
    fn the_initial_migration_declares_the_types_the_adapters_decode() {
        let generated = all_files(true);
        let sql = contents(&generated, "migrations/0001_tasks.sql");
        assert!(sql.contains("id    UUID PRIMARY KEY NOT NULL"), "{sql}");
        assert!(
            sql.contains("done  BOOLEAN NOT NULL DEFAULT FALSE"),
            "{sql}"
        );
        assert!(sql.contains("CREATE TABLE IF NOT EXISTS tasks"), "{sql}");
        assert!(!sql.contains("created_at"), "RF-187 y V15 siguen abiertas");
    }

    /// RF-003: sin módulo de ejemplo no hay migración que generar, y git no
    /// trackea directorios vacíos, así que `.gitkeep` es la única excepción.
    #[test]
    fn without_the_example_migrations_only_keeps_a_gitkeep() {
        let generated = all_files(false);
        let declared: BTreeSet<&Path> = generated.iter().map(|file| file.path.as_path()).collect();
        assert!(declared.contains(Path::new("migrations/.gitkeep")));
        assert!(!declared.contains(Path::new("migrations/0001_tasks.sql")));
    }

    #[test]
    fn no_folder_is_left_empty() {
        let generated = all_files(true);
        let declared: BTreeSet<&Path> = generated.iter().map(|file| file.path.as_path()).collect();

        // RF-192: con módulo de ejemplo la carpeta ya tiene contenido real.
        assert!(declared.contains(Path::new("migrations/0001_tasks.sql")));
        assert!(!declared.contains(Path::new("migrations/.gitkeep")));

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

    /// RF-190: los dos TOML que generan tienen que poder leerse. La plantilla de
    /// `rdd.toml` escapaba las comillas dentro de un raw string y emitía
    /// `migrations_dir = \"migrations\"`, que `rdd migrate` rechazaba.
    #[test]
    fn generated_toml_files_parse() {
        for engine in [Database::Sqlite, Database::Postgres] {
            let dependency = rdd_dependency(None, engine);
            let ctx = ProjectContext {
                engine,
                rdd_dependency: &dependency,
                ..context()
            };
            let generated = files_for(&ctx, true);
            for path in ["rdd.toml", "Cargo.toml"] {
                let text = contents(&generated, path);
                toml::from_str::<toml::Value>(text)
                    .unwrap_or_else(|err| panic!("{path} con {engine:?} no es TOML válido: {err}"));
            }
            let config = toml::from_str::<toml::Value>(contents(&generated, "rdd.toml")).unwrap();
            assert_eq!(
                config["database"]["migrations_dir"].as_str(),
                Some("migrations")
            );
            assert_eq!(
                config["database"]["provider"].as_str(),
                Some(engine.feature())
            );
        }
    }

    /// RF-189: el motor tiene que quedar coherente en los cinco sitios que lo
    /// nombran, o el proyecto no arranca sin edición manual (RF-143).
    #[test]
    fn postgres_is_consistent_across_the_generated_files() {
        let dependency = rdd_dependency(None, Database::Postgres);
        let ctx = ProjectContext {
            engine: Database::Postgres,
            rdd_dependency: &dependency,
            ..context()
        };
        let generated = files_for(&ctx, true);
        let url = "postgres://postgres:CAMBIAR@localhost:5432/todo_app";

        assert!(contents(&generated, "Cargo.toml").contains("features = [\"postgres\"]"));
        assert!(contents(&generated, "rdd.toml").contains("provider = \"postgres\""));
        assert!(contents(&generated, "prisma/schema.prisma").contains("provider = \"postgresql\""));
        for path in [".env", ".env.example", "src/config/database_config.rs"] {
            assert!(
                contents(&generated, path).contains(url),
                "{path} no lleva la URL de respaldo de PostgreSQL"
            );
        }
        assert!(contents(&generated, "src/main.rs").contains("config::database_config::url()"));
    }

    #[test]
    fn sqlite_is_the_default_and_needs_no_extra_feature() {
        let generated = all_files(true);
        assert!(contents(&generated, "Cargo.toml").contains("rdd = \"0.1\""));
        assert!(contents(&generated, "rdd.toml").contains("provider = \"sqlite\""));
        assert!(contents(&generated, "prisma/schema.prisma").contains("provider = \"sqlite\""));
        assert!(
            contents(&generated, "src/config/database_config.rs").contains("sqlite://todo-app.db")
        );
    }

    /// RF-063: `rdd migrate` crea `<paquete>.db` en la raíz (RF-192) con cuentas
    /// y tareas dentro; sin estas líneas la base queda versionable.
    #[test]
    fn the_gitignore_excludes_secrets_and_the_local_sqlite_database() {
        for engine in [Database::Sqlite, Database::Postgres] {
            let dependency = rdd_dependency(None, engine);
            let ctx = ProjectContext {
                engine,
                rdd_dependency: &dependency,
                ..context()
            };
            let generated = files_for(&ctx, true);
            let ignored = contents(&generated, ".gitignore");
            for pattern in ["/target", ".env", "*.log"] {
                assert!(
                    ignored.lines().any(|line| line == pattern),
                    "{engine:?}: falta `{pattern}` en el .gitignore"
                );
            }
            for pattern in ["*.db", "*.db-shm", "*.db-wal"] {
                let expected = engine == Database::Sqlite;
                assert_eq!(
                    ignored.lines().any(|line| line == pattern),
                    expected,
                    "{engine:?}: `{pattern}` debería estar excluido = {expected}"
                );
            }
        }
    }

    /// RF-187 / V15: una columna temporal en el esquema generaría código que el
    /// adaptador rechaza al leer (§15.2).
    #[test]
    fn the_prisma_model_has_no_temporal_column() {
        let generated = all_files(true);
        let schema = contents(&generated, "prisma/schema.prisma");
        assert!(schema.contains("model Task"));
        assert!(!schema.contains("createdAt"), "{schema}");
        assert!(!schema.contains("DateTime"), "{schema}");
    }

    /// Un `@` suelto es legítimo —Prisma lo usa en `@id`/`@@map` y la URL de
    /// PostgreSQL en el separador de credenciales—, así que solo cuenta el par
    /// `@nombre@` con nombre de token.
    fn unreplaced_token(text: &str) -> Option<String> {
        let mut rest = text;
        while let Some(start) = rest.find('@') {
            let after = &rest[start + 1..];
            if let Some(end) = after.find('@') {
                let name = &after[..end];
                if !name.is_empty() && name.bytes().all(|b| b.is_ascii_lowercase() || b == b'_') {
                    return Some(format!("@{name}@"));
                }
            }
            rest = after;
        }
        None
    }

    #[test]
    fn leaves_no_project_token_unreplaced() {
        for engine in [Database::Sqlite, Database::Postgres] {
            let dependency = rdd_dependency(None, engine);
            let ctx = ProjectContext {
                engine,
                rdd_dependency: &dependency,
                ..context()
            };
            for file in files_for(&ctx, true) {
                assert_eq!(
                    unreplaced_token(&file.contents),
                    None,
                    "quedó un token sin sustituir en {} con {engine:?}",
                    file.path.display()
                );
            }
        }
    }

    #[test]
    fn the_dependency_names_the_feature_only_when_it_is_not_default() {
        assert_eq!(rdd_dependency(None, Database::Sqlite), "\"0.1\"");
        assert_eq!(
            rdd_dependency(None, Database::Postgres),
            "{ version = \"0.1\", features = [\"postgres\"] }"
        );
        let local = rdd_dependency(Some(Path::new(".")), Database::Postgres);
        assert!(local.starts_with("{ path = \""), "{local}");
        assert!(local.ends_with(", features = [\"postgres\"] }"), "{local}");
        assert!(!local.contains("version"), "{local}");
    }
}
