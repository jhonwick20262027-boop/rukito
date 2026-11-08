//! SQL dinámico del adaptador: identificadores citados y valores enlazados.

use rdd_core::error::{Error, Result};

/// Cita un identificador y duplica las comillas internas.
pub fn quote(ident: &str) -> String {
    format!("\"{}\"", ident.replace('"', "\"\""))
}

/// `SELECT * FROM "tabla"`.
pub fn select_all(table: &str) -> String {
    format!("SELECT * FROM {}", quote(table))
}

/// `SELECT * FROM "tabla" WHERE "id" = ? LIMIT 1`.
pub fn select_by_id(table: &str, id_column: &str) -> String {
    format!(
        "SELECT * FROM {} WHERE {} = ? LIMIT 1",
        quote(table),
        quote(id_column)
    )
}

/// `DELETE FROM "tabla" WHERE "id" = ?`.
pub fn delete_by_id(table: &str, id_column: &str) -> String {
    format!(
        "DELETE FROM {} WHERE {} = ?",
        quote(table),
        quote(id_column)
    )
}

/// Upsert SQLite atómico por la primera columna, que identifica la fila.
pub fn upsert(table: &str, columns: &[&str], value_count: usize) -> Result<String> {
    if columns.is_empty() {
        return Err(Error::internal(anyhow::anyhow!(
            "`save` necesita al menos la columna del identificador (§12.1)"
        )));
    }
    if columns.len() != value_count {
        return Err(Error::internal(anyhow::anyhow!(
            "`columns` ({}) y `values` ({value_count}) deben tener la misma longitud (§12.2)",
            columns.len()
        )));
    }

    let names = columns
        .iter()
        .map(|column| quote(column))
        .collect::<Vec<_>>()
        .join(", ");
    let slots = vec!["?"; columns.len()].join(", ");
    let id = quote(columns[0]);
    let mut statement = format!(
        "INSERT INTO {} ({names}) VALUES ({slots}) ON CONFLICT ({id}) ",
        quote(table)
    );

    if columns.len() == 1 {
        statement.push_str("DO NOTHING");
    } else {
        let assignments = columns[1..]
            .iter()
            .map(|column| {
                let name = quote(column);
                format!("{name} = excluded.{name}")
            })
            .collect::<Vec<_>>()
            .join(", ");
        statement.push_str("DO UPDATE SET ");
        statement.push_str(&assignments);
    }
    Ok(statement)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cita_y_escapa_identificadores() {
        assert_eq!(quote("tasks"), "\"tasks\"");
        assert_eq!(quote("we\"ird"), "\"we\"\"ird\"");
    }

    #[test]
    fn upsert_usa_excluded_y_parametros_sqlite() {
        assert_eq!(
            upsert("tasks", &["id", "title"], 2).unwrap(),
            "INSERT INTO \"tasks\" (\"id\", \"title\") VALUES (?, ?) \
             ON CONFLICT (\"id\") DO UPDATE SET \"title\" = excluded.\"title\""
        );
    }

    #[test]
    fn upsert_solo_id_y_argumentos_desalineados() {
        assert_eq!(
            upsert("tasks", &["id"], 1).unwrap(),
            "INSERT INTO \"tasks\" (\"id\") VALUES (?) ON CONFLICT (\"id\") DO NOTHING"
        );
        assert!(upsert("tasks", &[], 0).is_err());
        assert!(upsert("tasks", &["id", "title"], 1).is_err());
    }
}
