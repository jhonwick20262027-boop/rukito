//! RDD0303: el prefijo de `#[controller]` debe empezar por `/` (§8.2, §8.7).

#![allow(dead_code, unused_imports, unused_variables)]

use rdd::controller;

pub struct C;

#[controller("c")]
impl C {
    #[get("/")]
    async fn list() -> rdd::Result<u32> {
        Ok(1)
    }
}

fn main() {}
