//! RDD0303: la ruta de un verbo debe empezar por `/` (§8.2, §8.7).

#![allow(dead_code, unused_imports, unused_variables)]

use rdd::controller;

pub struct C;

#[controller("/c")]
impl C {
    #[get("recursos")]
    async fn list() -> rdd::Result<u32> {
        Ok(1)
    }
}

fn main() {}
