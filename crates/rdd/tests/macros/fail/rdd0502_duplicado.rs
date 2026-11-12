//! RDD0502: dos campos publican el mismo nombre (§10.9).

#![allow(dead_code)]

use rdd::GraphqlObject;

#[derive(GraphqlObject)]
pub struct Tarea {
    task_id: String,
    #[graphql(name = "taskId")]
    other: String,
}

fn main() {}
