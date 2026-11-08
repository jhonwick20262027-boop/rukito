//! Ejecución de migraciones versionadas antes de iniciar o desplegar la app.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::Args;
use serde::Deserialize;
use sqlx::any::{AnyPoolOptions, install_default_drivers};
use sqlx::migrate::{MigrateDatabase, Migrator};

use crate::files;

#[derive(Args)]
pub struct MigrateArgs {
    /// URL de la base; por defecto se lee de la variable indicada en rdd.toml.
    #[arg(long)]
    pub database_url: Option<String>,

    /// Directorio de migraciones SQL (por defecto, `migrations/`).
    #[arg(long)]
    pub source: Option<PathBuf>,
}

#[derive(Default, Deserialize)]
struct ProjectConfig {
    #[serde(default)]
    database: DatabaseConfig,
}

#[derive(Default, Deserialize)]
struct DatabaseConfig {
    url_env: Option<String>,
    migrations_dir: Option<PathBuf>,
}

pub fn execute(args: MigrateArgs) -> Result<()> {
    let root = files::project_root()?;
    let dotenv = root.join(".env");
    if dotenv.is_file() {
        dotenvy::from_path(&dotenv)
            .with_context(|| format!("no se pudo cargar {}", dotenv.display()))?;
    }

    let config_path = root.join("rdd.toml");
    let config = if config_path.is_file() {
        let contents = std::fs::read_to_string(&config_path)
            .with_context(|| format!("no se pudo leer {}", config_path.display()))?;
        toml::from_str::<ProjectConfig>(&contents)
            .with_context(|| format!("configuración inválida en {}", config_path.display()))?
    } else {
        ProjectConfig::default()
    };

    let url_env = config.database.url_env.as_deref().unwrap_or("DATABASE_URL");
    let database_url = args
        .database_url
        .or_else(|| std::env::var(url_env).ok())
        .filter(|url| !url.trim().is_empty())
        .with_context(|| {
            format!("falta la URL de base de datos; define `{url_env}` o usa `--database-url`")
        })?;

    let source = args.source.unwrap_or_else(|| {
        config
            .database
            .migrations_dir
            .unwrap_or_else(|| PathBuf::from("migrations"))
    });
    let source = resolve_from_root(&root, &source);
    if !source.is_dir() {
        bail!(
            "no existe el directorio de migraciones: {}",
            source.display()
        );
    }

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("no se pudo crear el runtime Tokio para las migraciones")?;
    runtime.block_on(run(&database_url, &source))
}

async fn run(database_url: &str, source: &Path) -> Result<()> {
    install_default_drivers();
    let migrator = Migrator::new(source).await.with_context(|| {
        format!(
            "no se pudieron cargar migraciones desde {}",
            source.display()
        )
    })?;
    if database_url.starts_with("sqlite:")
        && !sqlx::Any::database_exists(database_url)
            .await
            .context("no se pudo comprobar el archivo SQLite")?
    {
        sqlx::Any::create_database(database_url)
            .await
            .context("no se pudo crear el archivo SQLite")?;
    }
    let pool = AnyPoolOptions::new()
        .connect(database_url)
        .await
        .context("no se pudo conectar a la base de datos configurada")?;
    let mut connection = pool
        .acquire()
        .await
        .context("no se pudo obtener una conexión para consultar migraciones")?;
    sqlx::migrate::Migrate::ensure_migrations_table(&mut *connection, "_sqlx_migrations")
        .await
        .context("no se pudo inicializar el historial de migraciones")?;
    let applied =
        sqlx::migrate::Migrate::list_applied_migrations(&mut *connection, "_sqlx_migrations")
            .await
            .context("no se pudo consultar el historial de migraciones")?;
    drop(connection);
    let applied_versions: std::collections::HashSet<i64> = applied
        .into_iter()
        .map(|migration| migration.version)
        .collect();
    let pending: Vec<_> = migrator
        .iter()
        .filter(|migration| !applied_versions.contains(&migration.version))
        .collect();

    migrator
        .run(&pool)
        .await
        .context("falló la ejecución de migraciones")?;

    if pending.is_empty() {
        println!("la base de datos ya está al día");
    } else {
        for migration in pending {
            println!(
                "migración aplicada: {} ({})",
                migration.version, migration.description
            );
        }
    }
    pool.close().await;
    Ok(())
}

fn resolve_from_root(root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_owned()
    } else {
        root.join(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn las_rutas_relativas_se_resuelven_desde_el_proyecto() {
        let root = std::env::temp_dir().join("rdd-migrations-project");
        assert_eq!(
            resolve_from_root(&root, Path::new("migrations")),
            root.join("migrations")
        );
    }

    #[test]
    fn las_rutas_absolutas_no_se_modifican() {
        let path = std::env::temp_dir().join("rdd-migrations-absolute");
        assert_eq!(resolve_from_root(&std::env::temp_dir(), &path), path);
    }
}
