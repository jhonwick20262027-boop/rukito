//! RDD0501: un campo con `i64` no tiene escalar GraphQL (§10.9).

#![allow(dead_code)]

use rdd::GraphqlObject;

#[derive(GraphqlObject)]
pub struct Contador {
    total: i64,
}

fn main() {}
