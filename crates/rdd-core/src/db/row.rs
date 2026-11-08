//! Fila neutra (§12.1): columna → valor, en el orden de `Persist::COLUMNS`.

use crate::db::Value;
use crate::error::{Error, Result};

/// Una fila leída del motor, sin tipos de ningún driver.
///
/// Conserva el orden de inserción, pero el orden **no es contractual**: `all` hace
/// `SELECT *` en los motores SQL y la fila llega en el orden físico de la tabla.
/// `Persist::from_row` lee por nombre y las columnas de más se ignoran (§12.1).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Row {
    pairs: Vec<(String, Value)>,
}

impl Row {
    /// Fila vacía.
    pub fn new() -> Self {
        Self::default()
    }

    /// Fila a partir de pares columna/valor ya ordenados.
    pub fn from_pairs(pairs: Vec<(String, Value)>) -> Self {
        Self { pairs }
    }

    /// Fila a partir de columnas y valores paralelos, como los recibe `save`.
    pub fn zip(columns: &[&str], values: &[Value]) -> Result<Self> {
        if columns.len() != values.len() {
            return Err(Error::internal(anyhow::anyhow!(
                "`columns` ({}) y `values` ({}) deben tener la misma longitud (§12.2)",
                columns.len(),
                values.len()
            )));
        }
        Ok(Self {
            pairs: columns
                .iter()
                .zip(values)
                .map(|(c, v)| ((*c).to_owned(), v.clone()))
                .collect(),
        })
    }

    /// Añade o sustituye una columna, conservando la posición si ya existía.
    pub fn insert(&mut self, column: impl Into<String>, value: Value) {
        let column = column.into();
        match self.pairs.iter_mut().find(|(c, _)| *c == column) {
            Some(slot) => slot.1 = value,
            None => self.pairs.push((column, value)),
        }
    }

    /// Valor de una columna, si existe.
    pub fn get(&self, column: &str) -> Option<&Value> {
        self.pairs.iter().find(|(c, _)| c == column).map(|(_, v)| v)
    }

    /// Valor de una columna, o error nombrándola (lo usa `Persist::from_row`).
    pub fn value(&self, column: &str) -> Result<&Value> {
        self.get(column).ok_or_else(|| {
            Error::internal(anyhow::anyhow!(
                "la fila no trae la columna `{column}`; columnas presentes: {}",
                self.columns().collect::<Vec<_>>().join(", ")
            ))
        })
    }

    /// Nombres de columna, en orden.
    pub fn columns(&self) -> impl Iterator<Item = &str> {
        self.pairs.iter().map(|(c, _)| c.as_str())
    }

    /// Valores, en el mismo orden que `columns`.
    pub fn values(&self) -> impl Iterator<Item = &Value> {
        self.pairs.iter().map(|(_, v)| v)
    }

    /// Número de columnas.
    pub fn len(&self) -> usize {
        self.pairs.len()
    }

    /// Cierto si no hay columnas.
    pub fn is_empty(&self) -> bool {
        self.pairs.is_empty()
    }

    /// Consume la fila y devuelve sus pares.
    pub fn into_pairs(self) -> Vec<(String, Value)> {
        self.pairs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zip_mantiene_el_orden() {
        let row = Row::zip(&["id", "title"], &[Value::I64(1), Value::from("x")]).unwrap();
        assert_eq!(row.columns().collect::<Vec<_>>(), ["id", "title"]);
        assert_eq!(row.value("title").unwrap().as_text().unwrap(), "x");
    }

    #[test]
    fn longitudes_distintas_son_error() {
        let err = Row::zip(&["id"], &[]).unwrap_err();
        assert!(err.to_string().contains("misma longitud"), "{err}");
    }

    #[test]
    fn insert_sustituye_sin_duplicar() {
        let mut row = Row::from_pairs(vec![("id".to_owned(), Value::I64(1))]);
        row.insert("id", Value::I64(2));
        row.insert("done", Value::Bool(true));
        assert_eq!(row.len(), 2);
        assert_eq!(row.value("id").unwrap().as_i64().unwrap(), 2);
    }

    #[test]
    fn una_columna_ausente_nombra_las_presentes() {
        let row = Row::from_pairs(vec![("id".to_owned(), Value::Null)]);
        let err = row.value("title").unwrap_err();
        assert!(err.to_string().contains("title"), "{err}");
        assert!(err.to_string().contains("id"), "{err}");
    }
}
