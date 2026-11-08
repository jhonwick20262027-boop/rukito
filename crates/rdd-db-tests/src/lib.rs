//! # rdd-db-tests
//!
//! Suite de conformidad de adaptadores de base de datos (§12.4, RF-180 a RF-182).
//!
//! Mismo razonamiento que ADR-15 para las plataformas: una abstracción con un solo
//! adaptador acaba copiando la forma de ese adaptador. Aquí **todo motor pasa los
//! mismos casos** sobre el mismo agregado de sonda, así que el puerto no se diseña
//! a imagen de PostgreSQL.
//!
//! ```ignore
//! #[tokio::test]
//! async fn conformidad() {
//!     let adapter = rdd_postgres::connect(&url).await.unwrap();
//!     // La tabla `rdd_db_probe` debe existir y estar vacía: la suite no hace DDL.
//!     let report = rdd_db_tests::run_all(adapter, rdd_postgres::unsupported()).await;
//!     assert!(report.skipped.is_empty());
//! }
//! ```
//!
//! Contra memoria no hace falta servidor (RF-181): [`run_all_memory`].

mod cases;

pub mod fixture;

use std::sync::Arc;

use fixture::Probe;
use rdd_core::db::{DbAdapter, MemoryBackend, Repo, Value};

/// Una variante de `Value` que el motor no soporta, declarada por su adaptador
/// (RF-182). `sample` solo se usa por su variante; el motivo se anuncia al saltar.
pub struct Unsupported {
    /// Valor de ejemplo de la variante no soportada.
    pub sample: Value,
    /// Por qué el motor no la soporta. Se imprime, nunca se silencia.
    pub reason: &'static str,
}

impl Unsupported {
    /// Declaración de una variante no soportada.
    pub fn new(sample: Value, reason: &'static str) -> Self {
        Self { sample, reason }
    }
}

/// Lo que corrió la suite: útil para que CI distinga conformidad de conexión
/// (RF-181) y para que un salto quede visible en el registro.
pub struct Report {
    /// Nombre del motor bajo prueba.
    pub engine: &'static str,
    /// Casos que pasaron.
    pub passed: usize,
    /// Casos saltados, cada uno con su motivo explícito.
    pub skipped: Vec<String>,
}

/// Corre los casos D-01 a D-09 contra un adaptador ya conectado.
///
/// La tabla [`fixture::TABLE`] debe existir y estar **vacía**: el puerto no tiene
/// DDL y `.run()` no aplica migraciones (RF-117). Cada caso limpia sus filas; si
/// uno falla a mitad, hay que truncar la tabla antes de reintentar.
///
/// # Panics
///
/// Si algún caso no cumple el contrato. Un fallo de conexión llega como `Err` del
/// `connect` del adaptador, antes de entrar aquí, y por eso CI puede distinguir
/// "no hay servidor" de "el motor no cumple el contrato" (RF-181).
pub async fn run_all(adapter: Arc<dyn DbAdapter>, unsupported: Vec<Unsupported>) -> Report {
    let engine = adapter.name();
    let repo = Repo::<Probe>::new(adapter);

    cases::empty_table(&repo).await;
    cases::round_trip(&repo).await;
    cases::upsert_without_duplicate(&repo).await;
    cases::get_missing_is_not_found(&repo).await;
    cases::delete_missing_is_not_found(&repo).await;
    cases::find_and_exists(&repo).await;
    cases::delete_removes(&repo).await;
    cases::misaligned_save_is_error(&repo).await;
    let skipped = cases::value_variants(&repo, &unsupported).await;

    let report = Report {
        engine,
        passed: 8 + (8 - skipped.len()),
        skipped,
    };
    eprintln!(
        "rdd-db-tests: {} pasó {} casos, saltó {}",
        report.engine,
        report.passed,
        report.skipped.len()
    );
    report
}

/// La suite contra `MemoryBackend`, sin servidor ni tabla que crear (RF-181).
///
/// Es la referencia del puerto: si memoria y un motor real divergen, el fallo es
/// del adaptador, nunca del test.
pub async fn run_all_memory() -> Report {
    run_all(Arc::new(MemoryBackend::new()), Vec::new()).await
}
