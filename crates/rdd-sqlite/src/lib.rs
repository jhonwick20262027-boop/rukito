//! # rdd-sqlite
//!
//! Adaptador SQLite del puerto [`DbAdapter`] (§12), sobre `sqlx` 0.9.
//! SQLite se habilita por defecto desde la fachada `rdd`; los proyectos nuevos
//! usan un archivo local y pueden elegir otro motor mediante `DATABASE_URL`.
//!
//! ```ignore
//! let adapter = rdd_sqlite::SqliteAdapter::connect("sqlite://rdd.db").await?;
//! let app = App::new().adapter(adapter.into_arc());
//! ```
//!
//! [`register`] instala la factoría de `sqlite://`; la fachada la registra al
//! arrancar con `#[rdd::main]`.

mod codec;
mod error;
mod sql;

use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use rdd_core::db::{DbAdapter, Row, Value, declare, install};
use rdd_core::error::{Error, Result};
use rdd_core::http::BoxFuture;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};
use sqlx::{AssertSqlSafe, Row as _, Sqlite};

/// Conexiones del pool por defecto.
pub const DEFAULT_MAX_CONNECTIONS: u32 = 10;

/// Espera por defecto para tomar una conexión del pool.
pub const DEFAULT_ACQUIRE_TIMEOUT: Duration = Duration::from_secs(5);

/// Pool SQLite conectado.
pub struct SqliteAdapter {
    pool: SqlitePool,
}

impl SqliteAdapter {
    async fn column_types(&self, table: &str) -> Result<HashMap<String, String>> {
        let rows = sqlx::query("SELECT name, type FROM pragma_table_info(?)")
            .bind(table)
            .fetch_all(&self.pool)
            .await
            .map_err(error::map)?;
        rows.iter()
            .map(|row| {
                let name = row.try_get("name").map_err(error::map)?;
                let type_name = row.try_get("type").map_err(error::map)?;
                Ok((name, type_name))
            })
            .collect()
    }

    /// Conecta con el pool por defecto y crea el archivo si todavía no existe.
    pub async fn connect(url: &str) -> Result<Self> {
        Self::connect_with(url, DEFAULT_MAX_CONNECTIONS, DEFAULT_ACQUIRE_TIMEOUT).await
    }

    /// Conecta fijando el pool.
    pub async fn connect_with(
        url: &str,
        max_connections: u32,
        acquire_timeout: Duration,
    ) -> Result<Self> {
        let options = SqliteConnectOptions::from_str(url)
            .map_err(|err| Error::internal(anyhow::Error::new(err)))?
            .create_if_missing(true)
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(max_connections)
            .acquire_timeout(acquire_timeout)
            .connect_with(options)
            .await
            .map_err(error::map)?;
        Ok(Self { pool })
    }

    /// Handle nativo para consultas específicas de SQLite.
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// El adaptador como trait object para `App::adapter`.
    pub fn into_arc(self) -> Arc<dyn DbAdapter> {
        Arc::new(self)
    }
}

impl DbAdapter for SqliteAdapter {
    fn name(&self) -> &'static str {
        "sqlite"
    }

    fn get<'a>(
        &'a self,
        table: &'a str,
        id_column: &'a str,
        id: &'a Value,
    ) -> BoxFuture<'a, Result<Option<Row>>> {
        Box::pin(async move {
            let query = codec::bind(
                sqlx::query::<Sqlite>(AssertSqlSafe(sql::select_by_id(table, id_column))),
                id,
            );
            let row = query.fetch_optional(&self.pool).await.map_err(error::map)?;
            let column_types = self.column_types(table).await?;
            row.as_ref()
                .map(|row| codec::row(row, &column_types))
                .transpose()
        })
    }

    fn all<'a>(&'a self, table: &'a str) -> BoxFuture<'a, Result<Vec<Row>>> {
        Box::pin(async move {
            let column_types = self.column_types(table).await?;
            let rows = sqlx::query::<Sqlite>(AssertSqlSafe(sql::select_all(table)))
                .fetch_all(&self.pool)
                .await
                .map_err(error::map)?;
            rows.iter()
                .map(|row| codec::row(row, &column_types))
                .collect()
        })
    }

    fn save<'a>(
        &'a self,
        table: &'a str,
        columns: &'a [&'a str],
        values: &'a [Value],
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let statement = sql::upsert(table, columns, values.len())?;
            let mut query = sqlx::query::<Sqlite>(AssertSqlSafe(statement));
            for value in values {
                query = codec::bind(query, value);
            }
            query.execute(&self.pool).await.map_err(error::map)?;
            Ok(())
        })
    }

    fn delete<'a>(
        &'a self,
        table: &'a str,
        id_column: &'a str,
        id: &'a Value,
    ) -> BoxFuture<'a, Result<bool>> {
        Box::pin(async move {
            let query = codec::bind(
                sqlx::query::<Sqlite>(AssertSqlSafe(sql::delete_by_id(table, id_column))),
                id,
            );
            let result = query.execute(&self.pool).await.map_err(error::map)?;
            Ok(result.rows_affected() > 0)
        })
    }
}

/// Instala la factoría de `sqlite://` en el registro (§12.6).
pub fn register() {
    declare(&["sqlite"], "rdd-sqlite", "sqlite");
    install("sqlite", factory);
}

fn factory(url: String) -> BoxFuture<'static, Result<Arc<dyn DbAdapter>>> {
    Box::pin(async move { Ok(SqliteAdapter::connect(&url).await?.into_arc()) })
}
