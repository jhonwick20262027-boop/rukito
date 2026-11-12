//! RDD0502: `__` está reservado para la introspección (§10.9).

#![allow(dead_code)]

use rdd::GraphqlObject;

#[derive(GraphqlObject)]
pub struct Tarea {
    #[graphql(name = "__typename")]
    nombre: String,
}

fn main() {}
