//! RDD0301: un método dentro de `#[controller]` sin atributo de verbo (§8.7).

#![allow(dead_code, unused_imports, unused_variables)]

use rdd::controller;

pub struct C;

#[controller("/c")]
impl C {
    async fn helper() -> rdd::Result<u32> {
        Ok(1)
    }
}

fn main() {}
