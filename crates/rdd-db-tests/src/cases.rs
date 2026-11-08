//! Los casos de conformidad (RF-180). Los mismos para cualquier motor.

use rdd_core::db::{Persist, Repo, Value};
use rdd_core::error::Error;
use uuid::Uuid;

use crate::Unsupported;
use crate::fixture::Probe;

/// D-01: `all` sobre una tabla vacía devuelve un vector vacío, no un error.
pub async fn empty_table(repo: &Repo<Probe>) {
    let all = repo.all().await.expect("D-01: all sobre tabla vacía");
    assert!(
        all.is_empty(),
        "D-01: la tabla de sonda debe estar vacía al empezar"
    );
}

/// D-02: `save` + `get` devuelven el agregado idéntico, campo por campo.
pub async fn round_trip(repo: &Repo<Probe>) {
    let probe = Probe::sample("D-02");
    repo.save(&probe).await.expect("D-02: save");
    let read = repo.get(probe.id).await.expect("D-02: get");
    assert_eq!(read, probe, "D-02: el agregado cambió en el round-trip");
    repo.delete(probe.id).await.expect("D-02: limpieza");
}

/// D-03 (RF-112): guardar dos veces el mismo id actualiza y NO duplica.
pub async fn upsert_without_duplicate(repo: &Repo<Probe>) {
    let mut probe = Probe::sample("D-03 antes");
    repo.save(&probe).await.expect("D-03: primer save");
    probe.label = "D-03 después".to_owned();
    probe.flag = false;
    repo.save(&probe).await.expect("D-03: segundo save");

    let all = repo.all().await.expect("D-03: all");
    let own: Vec<_> = all.iter().filter(|p| p.id == probe.id).collect();
    assert_eq!(own.len(), 1, "D-03: el upsert duplicó la fila (RF-112)");
    assert_eq!(own[0].label, "D-03 después");
    assert!(!own[0].flag);
    repo.delete(probe.id).await.expect("D-03: limpieza");
}

/// D-04: `get` de un id inexistente es `NotFound`, no `Internal` ni `None`.
pub async fn get_missing_is_not_found(repo: &Repo<Probe>) {
    let err = repo
        .get(Uuid::new_v4())
        .await
        .expect_err("D-04: get debía fallar");
    assert_not_found(err, "D-04");
}

/// D-05: `delete` de un id inexistente es `NotFound`.
pub async fn delete_missing_is_not_found(repo: &Repo<Probe>) {
    let err = repo
        .delete(Uuid::new_v4())
        .await
        .expect_err("D-05: delete debía fallar");
    assert_not_found(err, "D-05");
}

/// D-06: `find` y `exists` no fallan con un id ausente, y ven el que sí existe.
pub async fn find_and_exists(repo: &Repo<Probe>) {
    let id = Uuid::new_v4();
    assert!(repo.find(id).await.expect("D-06: find ausente").is_none());
    assert!(!repo.exists(id).await.expect("D-06: exists ausente"));

    let probe = Probe::sample("D-06");
    repo.save(&probe).await.expect("D-06: save");
    assert!(repo.exists(probe.id).await.expect("D-06: exists presente"));
    assert_eq!(
        repo.find(probe.id).await.expect("D-06: find presente"),
        Some(probe.clone())
    );
    repo.delete(probe.id).await.expect("D-06: limpieza");
}

/// D-07: `delete` borra de verdad; después `all` ya no la trae.
pub async fn delete_removes(repo: &Repo<Probe>) {
    let probe = Probe::sample("D-07");
    repo.save(&probe).await.expect("D-07: save");
    repo.delete(probe.id).await.expect("D-07: delete");
    assert!(repo.find(probe.id).await.expect("D-07: find").is_none());
    let all = repo.all().await.expect("D-07: all");
    assert!(
        all.iter().all(|p| p.id != probe.id),
        "D-07: la fila sobrevivió"
    );
}

/// D-08 (RF-180/RF-182): round-trip de cada variante de `Value`.
///
/// Devuelve los casos saltados con su motivo: un motor que no soporta una variante
/// lo declara y la suite lo anuncia, nunca lo pasa en silencio.
pub async fn value_variants(repo: &Repo<Probe>, unsupported: &[Unsupported]) -> Vec<String> {
    let mut skipped = Vec::new();
    for (variant, mutate) in VARIANTS {
        if let Some(declared) = unsupported
            .iter()
            .find(|u| u.sample.type_name() == *variant)
        {
            let msg = format!(
                "{variant}: {} (declarado por el adaptador)",
                declared.reason
            );
            eprintln!("  rdd-db-tests: SALTA {msg}");
            skipped.push(msg);
            continue;
        }
        let mut probe = Probe::sample(variant);
        mutate(&mut probe);
        repo.save(&probe)
            .await
            .unwrap_or_else(|err| panic!("D-08/{variant}: save: {err}"));
        let read = repo
            .get(probe.id)
            .await
            .unwrap_or_else(|err| panic!("D-08/{variant}: get: {err}"));
        assert_eq!(read, probe, "D-08/{variant}: la variante no sobrevivió");
        repo.delete(probe.id).await.expect("D-08: limpieza");
    }
    skipped
}

/// D-09: `save` con columnas y valores desalineados es un error, no un panic.
pub async fn misaligned_save_is_error(repo: &Repo<Probe>) {
    let adapter = repo.adapter();
    let err = adapter
        .save(
            Probe::TABLE,
            &["id"],
            &[Value::Uuid(Uuid::new_v4()), Value::Bool(true)],
        )
        .await
        .expect_err("D-09: save desalineado debía fallar");
    assert_eq!(
        err.status(),
        rdd_core::http::StatusCode::INTERNAL_SERVER_ERROR,
        "D-09: un desajuste de columnas es un fallo de esquema (RF-042): {err}"
    );
}

/// Una mutación por variante, con valores límite donde el motor suele romperse.
type Mutate = fn(&mut Probe);

const VARIANTS: &[(&str, Mutate)] = &[
    ("Uuid", set_uuid),
    ("Bool", set_bool),
    ("I64", set_i64),
    ("F64", set_f64),
    ("Text", set_text),
    ("Bytes", set_bytes),
    ("Json", set_json),
    ("Null", set_null),
];

fn set_uuid(p: &mut Probe) {
    p.id = Uuid::nil();
}

fn set_bool(p: &mut Probe) {
    p.flag = false;
}

fn set_i64(p: &mut Probe) {
    p.count = i64::MIN;
}

fn set_f64(p: &mut Probe) {
    p.ratio = -2.25;
}

fn set_text(p: &mut Probe) {
    p.label = "cañí, ünicode, 'comillas', \\ barra, 日本語".to_owned();
}

fn set_bytes(p: &mut Probe) {
    p.blob = vec![0, 255, 128, 1, 0];
}

fn set_json(p: &mut Probe) {
    p.doc = serde_json::json!({ "nested": { "x": null }, "arr": [true, 1.25, "s"] });
}

fn set_null(p: &mut Probe) {
    p.maybe = None;
}

fn assert_not_found(err: Error, case: &str) {
    assert_eq!(
        err.status(),
        rdd_core::http::StatusCode::NOT_FOUND,
        "{case}: se esperaba 404 NotFound (RF-040): {err}"
    );
}
