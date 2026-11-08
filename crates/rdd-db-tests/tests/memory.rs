//! RF-181: la suite corre contra memoria sin servidor. Es la referencia del puerto.

#[tokio::test]
async fn memoria_pasa_la_suite_completa() {
    let report = rdd_db_tests::run_all_memory().await;
    assert_eq!(report.engine, "memoria");
    assert_eq!(report.passed, 16);
    assert!(
        report.skipped.is_empty(),
        "memoria soporta las ocho variantes de Value: {:?}",
        report.skipped
    );
}

#[tokio::test]
async fn una_variante_declarada_se_salta_con_motivo() {
    use std::sync::Arc;

    use rdd_core::db::{MemoryBackend, Value};
    use rdd_db_tests::Unsupported;

    let report = rdd_db_tests::run_all(
        Arc::new(MemoryBackend::new()),
        vec![Unsupported::new(
            Value::F64(0.0),
            "prueba de RF-182: motor sin coma flotante",
        )],
    )
    .await;
    assert_eq!(report.passed, 15);
    assert_eq!(report.skipped.len(), 1);
    assert!(report.skipped[0].contains("F64"), "{:?}", report.skipped);
    assert!(
        report.skipped[0].contains("motor sin coma flotante"),
        "el motivo debe anunciarse, no silenciarse: {:?}",
        report.skipped
    );
}
