//! RDD0304: dos handlers con el mismo verbo y ruta (§8.7).

#![allow(dead_code, unused_imports, unused_variables)]

use rdd::controller;

pub struct C;

#[controller("/c")]
impl C {
    #[get("/")]
    async fn list() -> rdd::Result<u32> {
        Ok(1)
    }

    #[get("/")]
    async fn all() -> rdd::Result<u32> {
        Ok(2)
    }
}

fn main() {}
