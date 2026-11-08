//! Conformidad contra PostgreSQL real (RA-016, RF-180, RF-181).
//!
//! Se activa con `RDD_TEST_POSTGRES_URL`. Sin esa variable los tests se saltan con
//! razón explícita: la ausencia de servidor no es un fallo de conformidad (RF-181).
//! La tabla de sonda la crea y la destruye este fichero, porque el puerto no tiene
//! DDL y `.run()` no aplica migraciones (RF-117).

use rdd_core::db::Value;
use rdd_core::http::StatusCode;
use rdd_db_tests::fixture::TABLE;
use rdd_postgres::PostgresAdapter;
use sqlx::AssertSqlSafe;
use sqlx::postgres::PgPool;
use uuid::Uuid;

/// DDL de referencia de `rdd_db_tests::fixture` (§12.4).
const DDL: &str = "CREATE TABLE rdd_db_probe (
    id     UUID PRIMARY KEY,
    flag   BOOLEAN NOT NULL,
    count  BIGINT NOT NULL,
    ratio  DOUBLE PRECISION NOT NULL,
    label  TEXT NOT NULL,
    blob   BYTEA NOT NULL,
    doc    JSONB NOT NULL,
    maybe  TEXT
)";

async fn connect() -> Option<PostgresAdapter> {
    let Ok(url) = std::env::var("RDD_TEST_POSTGRES_URL") else {
        eprintln!("SALTA: RDD_TEST_POSTGRES_URL no está definida (RF-181)");
        return None;
    };
    Some(
        PostgresAdapter::connect(&url)
            .await
            .expect("PostgreSQL debe estar arriba para este test (RA-016)"),
    )
}

async fn execute(pool: &PgPool, statement: String) {
    sqlx::query(AssertSqlSafe(statement))
        .execute(pool)
        .await
        .expect("DDL de prueba");
}

#[tokio::test]
async fn conformidad_postgres() {
    let Some(adapter) = connect().await else {
        return;
    };
    let pool = adapter.pool().clone();

    execute(&pool, format!("DROP TABLE IF EXISTS {TABLE}")).await;
    execute(&pool, DDL.to_owned()).await;

    let report = rdd_db_tests::run_all(adapter.into_arc(), Vec::new()).await;

    execute(&pool, format!("DROP TABLE IF EXISTS {TABLE}")).await;

    assert_eq!(report.engine, "postgres");
    assert!(report.skipped.is_empty(), "{:?}", report.skipped);
    assert_eq!(report.passed, 16, "PostgreSQL soporta las ocho variantes");
}

#[tokio::test]
async fn null_inferido_json_y_tipos_sin_variante() {
    let Some(adapter) = connect().await else {
        return;
    };
    let pool = adapter.pool().clone();
    let table = "rdd_pg_extras";
    let id = Uuid::new_v4();

    execute(&pool, format!("DROP TABLE IF EXISTS {table}")).await;
    execute(
        &pool,
        format!("CREATE TABLE {table} (id UUID PRIMARY KEY, maybe UUID, doc JSON NOT NULL)"),
    )
    .await;

    let adapter = adapter.into_arc();
    let doc = serde_json::json!({ "a": [1, 2] });
    adapter
        .save(
            table,
            &["id", "maybe", "doc"],
            &[Value::Uuid(id), Value::Null, Value::Json(doc.clone())],
        )
        .await
        .expect("un NULL sin tipo debe inferirlo la columna UUID, y JSONB debe entrar en JSON");

    let row = adapter
        .get(table, "id", &Value::Uuid(id))
        .await
        .unwrap()
        .expect("la fila existe");
    assert!(row.value("maybe").unwrap().is_null());
    assert_eq!(row.value("doc").unwrap().as_json().unwrap(), &doc);

    // RF-187: una columna temporal no tiene variante en `Value`. Convertirla a
    // texto se leería bien y fallaría al volver a guardar, así que se rechaza.
    execute(
        &pool,
        format!("ALTER TABLE {table} ADD COLUMN stamp TIMESTAMPTZ NOT NULL DEFAULT NOW()"),
    )
    .await;
    let err = adapter.all(table).await.unwrap_err();
    let msg = err.to_string();
    assert_eq!(err.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert!(msg.contains("stamp"), "{msg}");
    assert!(msg.contains("TIMESTAMPTZ"), "{msg}");
    assert!(msg.contains("V15"), "{msg}");

    execute(&pool, format!("DROP TABLE IF EXISTS {table}")).await;
}
