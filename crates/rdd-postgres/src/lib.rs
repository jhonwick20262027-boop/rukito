//! # rdd-postgres
//!
//! Adaptador PostgreSQL del puerto [`DbAdapter`] (§12), sobre `sqlx` 0.9.
//!
//! Es la referencia del puerto: `save` es un upsert nativo en una sentencia
//! (RF-112), los identificadores van entrecomillados (RF-114) y los valores
//! siempre ligados (RF-113). No depende de ningún otro adaptador ni de ninguna
//! plataforma HTTP (RA-015), y solo se publica si pasa `rdd-db-tests` contra un
//! servidor real (RA-016).
//!
//! ```ignore
//! let adapter = rdd_postgres::PostgresAdapter::connect(&url).await?;
//! let app = App::new().adapter(adapter.into_arc());
//! ```
//!
//! Lo normal no es llamarlo a mano: [`register`] instala la factoría de los
//! esquemas `postgres://` y `postgresql://`, y `App::connect()` la resuelve (RF-111).

mod codec;
mod error;
mod sql;

use std::sync::Arc;
use std::time::Duration;

use rdd_core::db::{DbAdapter, Row, Value, declare, install};
use rdd_core::error::Result;
use rdd_core::http::BoxFuture;
use sqlx::postgres::{PgPool, PgPoolOptions};
use sqlx::{AssertSqlSafe, Postgres};

/// Conexiones del pool por defecto: el valor que §15.1 pone en `rdd.toml` (RF-116).
pub const DEFAULT_MAX_CONNECTIONS: u32 = 10;

/// Espera por defecto para tomar una conexión del pool (RF-116).
pub const DEFAULT_ACQUIRE_TIMEOUT: Duration = Duration::from_secs(5);

/// Pool de PostgreSQL ya conectado, listo para `App::adapter`.
pub struct PostgresAdapter {
    pool: PgPool,
}

impl PostgresAdapter {
    /// Conecta con el pool por defecto.
    pub async fn connect(url: &str) -> Result<Self> {
        Self::connect_with(url, DEFAULT_MAX_CONNECTIONS, DEFAULT_ACQUIRE_TIMEOUT).await
    }

    /// Conecta fijando el pool (RF-116).
    ///
    /// `url` es la de `DATABASE_URL`; el esquema ya lo resolvió el registro, así
    /// que aquí solo se conecta. El fallo llega antes de abrir el socket (RF-119).
    pub async fn connect_with(
        url: &str,
        max_connections: u32,
        acquire_timeout: Duration,
    ) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(max_connections)
            .acquire_timeout(acquire_timeout)
            .connect(url)
            .await
            .map_err(error::map)?;
        Ok(Self { pool })
    }

    /// Handle nativo para consultas que el puerto no cubre (RF-115).
    ///
    /// Ese código es específico de PostgreSQL y no portable: `rdd check` lo marca
    /// con RA-007 salvo que se suprima con motivo.
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// El adaptador como trait object, que es lo que guarda `App` y `Repo<T>`.
    pub fn into_arc(self) -> Arc<dyn DbAdapter> {
        Arc::new(self)
    }
}

impl DbAdapter for PostgresAdapter {
    fn name(&self) -> &'static str {
        "postgres"
    }

    fn get<'a>(
        &'a self,
        table: &'a str,
        id_column: &'a str,
        id: &'a Value,
    ) -> BoxFuture<'a, Result<Option<Row>>> {
        Box::pin(async move {
            let query = codec::bind(
                sqlx::query::<Postgres>(AssertSqlSafe(sql::select_by_id(table, id_column))),
                id,
            );
            let row = query.fetch_optional(&self.pool).await.map_err(error::map)?;
            row.as_ref().map(codec::row).transpose()
        })
    }

    fn all<'a>(&'a self, table: &'a str) -> BoxFuture<'a, Result<Vec<Row>>> {
        Box::pin(async move {
            let rows = sqlx::query::<Postgres>(AssertSqlSafe(sql::select_all(table)))
                .fetch_all(&self.pool)
                .await
                .map_err(error::map)?;
            rows.iter().map(codec::row).collect()
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
            let mut query = sqlx::query::<Postgres>(AssertSqlSafe(statement));
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
                sqlx::query::<Postgres>(AssertSqlSafe(sql::delete_by_id(table, id_column))),
                id,
            );
            let result = query.execute(&self.pool).await.map_err(error::map)?;
            Ok(result.rows_affected() > 0)
        })
    }
}

/// Instala la factoría de `postgres://` y `postgresql://` en el registro (§12.6).
///
/// La llama la fachada `rdd` al arrancar: el núcleo no puede conocer este crate
/// sin arrastrar un driver (RA-014), y declarar el esquema sin instalarlo es lo
/// que produce el mensaje de RF-111 nombrando `rdd-postgres`.
pub fn register() {
    declare(&["postgres", "postgresql"], "rdd-postgres", "postgres");
    install("postgres", factory);
    install("postgresql", factory);
}

fn factory(url: String) -> BoxFuture<'static, Result<Arc<dyn DbAdapter>>> {
    Box::pin(async move { Ok(PostgresAdapter::connect(&url).await?.into_arc()) })
}
