//! RDD0501: un campo raíz debe ser `async fn` (§10.9).

#![allow(dead_code)]

use rdd::graphql::graphql_object;

#[derive(Default)]
pub struct Query;

#[graphql_object(query)]
impl Query {
    fn total(&self, _ctx: &rdd::graphql::GraphqlContext) -> rdd::Result<u32> {
        Ok(1)
    }
}

fn main() {}
