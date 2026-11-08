//! Backend en memoria (§12.5, RF-015).
//!
//! Vive en el núcleo porque no arrastra ningún driver (RA-014). Es el backend de
//! los tests y de `rdd run --memory`; cada `App` recibe el suyo, así que no hay
//! estado global que compartir entre tests (RF-160).

use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard};

use crate::db::{DbAdapter, Row, Value};
use crate::error::{Error, Result};
use crate::http::BoxFuture;

/// Tabla → clave canónica del id → fila. `BTreeMap` da un `all()` determinista.
type Tables = BTreeMap<String, BTreeMap<String, Row>>;

/// Almacén en memoria, aislado por instancia.
#[derive(Default)]
pub struct MemoryBackend {
    tables: Mutex<Tables>,
}

impl MemoryBackend {
    /// Almacén vacío.
    pub fn new() -> Self {
        Self::default()
    }

    /// RF-073: un mutex envenenado no debe tumbar la app; los datos siguen siendo
    /// válidos porque ninguna operación deja la tabla a medias.
    fn lock(&self) -> MutexGuard<'_, Tables> {
        self.tables.lock().unwrap_or_else(|err| err.into_inner())
    }
}

impl DbAdapter for MemoryBackend {
    fn name(&self) -> &'static str {
        "memoria"
    }

    fn get<'a>(
        &'a self,
        table: &'a str,
        _id_column: &'a str,
        id: &'a Value,
    ) -> BoxFuture<'a, Result<Option<Row>>> {
        Box::pin(async move {
            let tables = self.lock();
            Ok(tables
                .get(table)
                .and_then(|rows| rows.get(&id.key()))
                .cloned())
        })
    }

    fn all<'a>(&'a self, table: &'a str) -> BoxFuture<'a, Result<Vec<Row>>> {
        Box::pin(async move {
            let tables = self.lock();
            Ok(tables
                .get(table)
                .map(|rows| rows.values().cloned().collect())
                .unwrap_or_default())
        })
    }

    fn save<'a>(
        &'a self,
        table: &'a str,
        columns: &'a [&'a str],
        values: &'a [Value],
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            if columns.is_empty() {
                return Err(Error::internal(anyhow::anyhow!(
                    "`save` necesita al menos la columna del identificador (§12.1)"
                )));
            }
            let row = Row::zip(columns, values)?;
            let key = values[0].key();
            self.lock()
                .entry(table.to_owned())
                .or_default()
                .insert(key, row);
            Ok(())
        })
    }

    fn delete<'a>(
        &'a self,
        table: &'a str,
        _id_column: &'a str,
        id: &'a Value,
    ) -> BoxFuture<'a, Result<bool>> {
        Box::pin(async move {
            let mut tables = self.lock();
            Ok(tables
                .get_mut(table)
                .and_then(|rows| rows.remove(&id.key()))
                .is_some())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const COLUMNS: &[&str] = &["id", "title"];

    fn values(id: i64, title: &str) -> Vec<Value> {
        vec![Value::I64(id), Value::from(title)]
    }

    #[tokio::test]
    async fn save_es_upsert_y_no_duplica() {
        let db = MemoryBackend::new();
        db.save("tasks", COLUMNS, &values(1, "uno")).await.unwrap();
        db.save("tasks", COLUMNS, &values(1, "dos")).await.unwrap();
        let all = db.all("tasks").await.unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].value("title").unwrap().as_text().unwrap(), "dos");
    }

    #[tokio::test]
    async fn get_de_un_id_ausente_es_none() {
        let db = MemoryBackend::new();
        assert!(
            db.get("tasks", "id", &Value::I64(9))
                .await
                .unwrap()
                .is_none()
        );
        assert!(db.all("tasks").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn delete_dice_si_existia() {
        let db = MemoryBackend::new();
        db.save("tasks", COLUMNS, &values(1, "uno")).await.unwrap();
        assert!(db.delete("tasks", "id", &Value::I64(1)).await.unwrap());
        assert!(!db.delete("tasks", "id", &Value::I64(1)).await.unwrap());
    }

    #[tokio::test]
    async fn dos_instancias_no_comparten_filas() {
        let a = MemoryBackend::new();
        let b = MemoryBackend::new();
        a.save("tasks", COLUMNS, &values(1, "uno")).await.unwrap();
        assert!(b.all("tasks").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn save_sin_columnas_es_error() {
        let db = MemoryBackend::new();
        let err = db.save("tasks", &[], &[]).await.unwrap_err();
        assert!(err.to_string().contains("identificador"), "{err}");
    }
}
