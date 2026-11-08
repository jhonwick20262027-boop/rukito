//! Conformidad SQLite contra una base efímera en memoria.

use rdd_core::db::{DbAdapter, Value};
use rdd_db_tests::fixture::TABLE;
use rdd_sqlite::SqliteAdapter;
use sqlx::AssertSqlSafe;
use sqlx::sqlite::SqlitePool;
use std::time::Duration;

const DDL: &str = "CREATE TABLE rdd_db_probe (
    id     UUID PRIMARY KEY,
    flag   BOOLEAN NOT NULL,
    count  BIGINT NOT NULL,
    ratio  DOUBLE PRECISION NOT NULL,
    label  TEXT NOT NULL,
    blob   BLOB NOT NULL,
    doc    JSON NOT NULL,
    maybe  TEXT
)";

async fn adapter() -> SqliteAdapter {
    let adapter = SqliteAdapter::connect_with("sqlite::memory:", 1, Duration::from_secs(5))
        .await
        .expect("SQLite en memoria debe conectar");
    sqlx::query(AssertSqlSafe(DDL.to_owned()))
        .execute(adapter.pool())
        .await
        .expect("crear tabla de conformidad");
    adapter
}

async fn execute(pool: &SqlitePool, statement: String) {
    sqlx::query(AssertSqlSafe(statement))
        .execute(pool)
        .await
        .expect("DDL de prueba");
}

#[tokio::test]
async fn conformidad_sqlite() {
    let adapter = adapter().await;
    let report = rdd_db_tests::run_all(adapter.into_arc(), Vec::new()).await;

    assert_eq!(report.engine, "sqlite");
    assert!(report.skipped.is_empty(), "{:?}", report.skipped);
    assert_eq!(report.passed, 16, "SQLite soporta las ocho variantes");
}

#[tokio::test]
async fn register_instala_el_esquema_sqlite() {
    rdd_sqlite::register();
    let factory = rdd_core::db::factory_for("sqlite://rdd.db")
        .expect("sqlite:// debe estar disponible tras el registro");
    let connected = factory("sqlite::memory:".to_owned())
        .await
        .expect("la factoría conecta una base en memoria");
    assert_eq!(connected.name(), "sqlite");
}

#[tokio::test]
async fn identificadores_y_valores_se_mantienen_separados() {
    let adapter = adapter().await;
    let id = uuid::Uuid::new_v4();
    adapter
        .save(
            TABLE,
            &[
                "id", "flag", "count", "ratio", "label", "blob", "doc", "maybe",
            ],
            &[
                Value::Uuid(id),
                Value::Bool(true),
                Value::I64(4),
                Value::F64(2.5),
                Value::from("a'; DROP TABLE tasks; --"),
                Value::Bytes(vec![0, 255]),
                Value::Json(serde_json::json!({"safe": true})),
                Value::Null,
            ],
        )
        .await
        .expect("valores enlazados se guardan como datos");

    let row = adapter
        .get(TABLE, "id", &Value::Uuid(id))
        .await
        .expect("lectura de la fila")
        .expect("fila existente");
    assert_eq!(
        row.value("label").unwrap().as_text().unwrap(),
        "a'; DROP TABLE tasks; --"
    );
    assert_eq!(
        row.value("doc").unwrap().as_json().unwrap(),
        &serde_json::json!({"safe": true})
    );
}

#[tokio::test]
async fn upsert_con_solo_id_no_falla() {
    let adapter = adapter().await;
    execute(
        adapter.pool(),
        "CREATE TABLE rdd_db_id_only (id UUID PRIMARY KEY)".to_owned(),
    )
    .await;
    let id = uuid::Uuid::new_v4();
    adapter
        .save("rdd_db_id_only", &["id"], &[Value::Uuid(id)])
        .await
        .expect("upsert de solo id");
    execute(adapter.pool(), "DROP TABLE rdd_db_id_only".to_owned()).await;
}
