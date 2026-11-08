//! Agregado `Task`.
//!
//! Los campos JSON conservan el nombre Rust (RF-071).

use rdd::{Aggregate, Persist, Result, Row, Value};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Task {
    pub id: Uuid,
    pub title: String,
    pub done: bool,
}

impl Aggregate for Task {
    type Id = Uuid;

    fn id(&self) -> &Self::Id {
        &self.id
    }
}

impl Persist for Task {
    const TABLE: &'static str = "tasks";
    const COLUMNS: &'static [&'static str] = &["id", "title", "done"];

    fn to_values(&self) -> Vec<Value> {
        vec![self.id.into(), self.title.clone().into(), self.done.into()]
    }

    fn from_row(row: &Row) -> Result<Self> {
        Ok(Self {
            id: row.value("id")?.as_uuid()?,
            title: row.value("title")?.as_text()?.to_owned(),
            done: row.value("done")?.as_bool()?,
        })
    }
}
