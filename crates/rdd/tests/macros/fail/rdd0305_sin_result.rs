//! RDD0305: el handler no devuelve `rdd::Result<T>` (§8.7).

#![allow(dead_code, unused_imports, unused_variables)]

use rdd::controller;

pub struct C;

#[controller("/c")]
impl C {
    #[get("/")]
    async fn list() -> u32 {
        1
    }
}

fn main() {}
