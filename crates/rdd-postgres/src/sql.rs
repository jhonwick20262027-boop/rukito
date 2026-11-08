//! SQL dinámico del adaptador: entrecomillado y sentencias (§12.5).
//!
//! Los identificadores se entrecomillan siempre (RF-114) y los valores NUNCA se
//! interpolan: van como parámetros `$n` (RF-113).
//!
//! sqlx 0.9 solo acepta `&'static str` sin auditar, así que el llamador envuelve
//! estas cadenas en `AssertSqlSafe`. La auditoría es esta: lo único dinámico que
//! entra aquí son nombres de tabla y de columna, y [`quote`] los entrecomilla
//! duplicando las comillas internas, que es el escape de PostgreSQL. Ningún valor
//! de usuario llega a este módulo.

use rdd_core::error::{Error, Result};

/// `"tabla"` con las comillas internas duplicadas, que es el escape de PostgreSQL.
pub fn quote(ident: &str) -> String {
    format!("\"{}\"", ident.replace('"', "\"\""))
}

/// `SELECT * FROM "tabla"` (RF-185: el orden de las columnas no es contractual).
pub fn select_all(table: &str) -> String {
    format!("SELECT * FROM {}", quote(table))
}

/// `SELECT * FROM "tabla" WHERE "id" = $1 LIMIT 1`.
pub fn select_by_id(table: &str, id_column: &str) -> String {
    format!(
        "SELECT * FROM {} WHERE {} = $1 LIMIT 1",
        quote(table),
        quote(id_column)
    )
}

/// `DELETE FROM "tabla" WHERE "id" = $1`.
pub fn delete_by_id(table: &str, id_column: &str) -> String {
    format!(
        "DELETE FROM {} WHERE {} = $1",
        quote(table),
        quote(id_column)
    )
}

/// Upsert nativo de PostgreSQL (RF-112): una sola sentencia atómica.
///
/// `INSERT … ON CONFLICT (id) DO UPDATE SET … = EXCLUDED.…`. Con una sola columna
/// —el identificador— no hay nada que actualizar, así que es `DO NOTHING`.
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
        .map(|c| quote(c))
        .collect::<Vec<_>>()
        .join(", ");
    let slots = (1..=columns.len())
        .map(|i| format!("${i}"))
        .collect::<Vec<_>>()
        .join(", ");
    let id = quote(columns[0]);

    let mut sql = format!(
        "INSERT INTO {} ({names}) VALUES ({slots}) ON CONFLICT ({id}) ",
        quote(table)
    );
    if columns.len() == 1 {
        sql.push_str("DO NOTHING");
    } else {
        let sets = columns[1..]
            .iter()
            .map(|c| {
                let name = quote(c);
                format!("{name} = EXCLUDED.{name}")
            })
            .collect::<Vec<_>>()
            .join(", ");
        sql.push_str("DO UPDATE SET ");
        sql.push_str(&sets);
    }
    Ok(sql)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entrecomilla_y_escapa() {
        assert_eq!(quote("tasks"), "\"tasks\"");
        assert_eq!(quote("we\"ird"), "\"we\"\"ird\"");
    }

    #[test]
    fn el_upsert_usa_excluded() {
        let sql = upsert("tasks", &["id", "title", "done"], 3).unwrap();
        assert_eq!(
            sql,
            "INSERT INTO \"tasks\" (\"id\", \"title\", \"done\") VALUES ($1, $2, $3) \
             ON CONFLICT (\"id\") DO UPDATE SET \"title\" = EXCLUDED.\"title\", \"done\" = EXCLUDED.\"done\""
        );
    }

    #[test]
    fn solo_el_id_es_do_nothing() {
        let sql = upsert("tasks", &["id"], 1).unwrap();
        assert!(sql.ends_with("ON CONFLICT (\"id\") DO NOTHING"), "{sql}");
    }

    #[test]
    fn longitudes_desalineadas_son_error() {
        let err = upsert("tasks", &["id", "title"], 1).unwrap_err();
        assert!(err.to_string().contains("misma longitud"), "{err}");
        assert!(upsert("tasks", &[], 0).is_err());
    }

    #[test]
    fn los_valores_nunca_se_interpolan() {
        let sql = select_by_id("tasks", "id");
        assert!(sql.contains("= $1"), "{sql}");
        assert!(!sql.contains("'; DROP"), "{sql}");
    }
}
