//! Agregado `Task` e invariantes del dominio (§7.1, §12.2, §26.5).
//!
//! `Aggregate` y `Persist` se escriben a mano hasta que `#[aggregate]` exista
//! (RF-006); la macro las sustituirá sin tocar el resto del módulo. El agregado no
//! deriva `Deserialize` (RF-031): la entrada HTTP son los DTOs, no él.
//!
//! `owner_id` lo añade el demo: cada cuenta ve solo sus tareas. La columna llega
//! en `migrations/0002_users.sql`, no en la inicial que genera el CLI (RF-192).

use rdd::db::{Persist, Row, Value};
use rdd::domain::Aggregate;
use uuid::Uuid;

pub const MAX_TITLE_LEN: usize = 200;

#[derive(Clone, Debug, PartialEq)]
pub struct Task {
    pub id: Uuid,
    pub title: String,
    pub done: bool,
    pub owner_id: Uuid,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TaskError {
    EmptyTitle,
    TitleTooLong { max: usize },
}

impl Task {
    /// Crea el agregado con un id nuevo, validando las invariantes. El dueño no
    /// es opcional: una tarea sin dueño no sería visible para nadie.
    pub fn new(owner_id: Uuid, title: String, done: bool) -> Result<Self, TaskError> {
        Ok(Self {
            id: Uuid::new_v4(),
            title: validate_title(title)?,
            done,
            owner_id,
        })
    }

    pub fn update(&mut self, title: Option<String>, done: Option<bool>) -> Result<(), TaskError> {
        if let Some(title) = title {
            self.title = validate_title(title)?;
        }
        if let Some(done) = done {
            self.done = done;
        }
        Ok(())
    }

    /// Si la tarea pertenece a esta identidad. Lo pregunta el caso de uso, no el
    /// controller (§26.5).
    pub fn belongs_to(&self, owner_id: Uuid) -> bool {
        self.owner_id == owner_id
    }
}

impl Aggregate for Task {
    type Id = Uuid;

    fn id(&self) -> &Self::Id {
        &self.id
    }
}

impl Persist for Task {
    const TABLE: &'static str = "tasks";
    const COLUMNS: &'static [&'static str] = &["id", "title", "done", "owner_id"];

    fn to_values(&self) -> Vec<Value> {
        vec![
            Value::Uuid(self.id),
            Value::from(self.title.clone()),
            Value::Bool(self.done),
            Value::Uuid(self.owner_id),
        ]
    }

    fn from_row(row: &Row) -> rdd::Result<Self> {
        // Por nombre y no por posición: el orden de `Row` no es contractual (§12.1).
        Ok(Self {
            id: row.value("id")?.as_uuid()?,
            title: row.value("title")?.as_text()?.to_owned(),
            done: row.value("done")?.as_bool()?,
            owner_id: row.value("owner_id")?.as_uuid()?,
        })
    }
}

fn validate_title(title: String) -> Result<String, TaskError> {
    let title = title.trim();
    if title.is_empty() {
        return Err(TaskError::EmptyTitle);
    }
    if title.chars().count() > MAX_TITLE_LEN {
        return Err(TaskError::TitleTooLong { max: MAX_TITLE_LEN });
    }
    Ok(title.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owner() -> Uuid {
        Uuid::new_v4()
    }

    #[test]
    fn recorta_el_titulo_y_rechaza_uno_vacio() {
        let item = Task::new(owner(), "  pan  ".to_owned(), false).unwrap();
        assert_eq!(item.title, "pan");
        assert_eq!(
            Task::new(owner(), "   ".to_owned(), false).unwrap_err(),
            TaskError::EmptyTitle
        );
    }

    #[test]
    fn rechaza_un_titulo_pasado_del_limite() {
        let long = "x".repeat(MAX_TITLE_LEN + 1);
        assert_eq!(
            Task::new(owner(), long, false).unwrap_err(),
            TaskError::TitleTooLong { max: MAX_TITLE_LEN }
        );
    }

    #[test]
    fn sobrevive_un_round_trip_por_row() {
        let item = Task::new(owner(), "pan".to_owned(), true).unwrap();
        let row = Row::zip(<Task as Persist>::COLUMNS, &item.to_values()).unwrap();
        assert_eq!(<Task as Persist>::from_row(&row).unwrap(), item);
    }

    #[test]
    fn el_dueño_es_parte_de_la_identidad_de_la_tarea() {
        let mine = owner();
        let item = Task::new(mine, "pan".to_owned(), false).unwrap();
        assert!(item.belongs_to(mine));
        assert!(!item.belongs_to(owner()));
    }
}
