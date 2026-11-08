//! RDD0301: un atributo de verbo fuera de `#[controller]` (§8.2, §8.7).

#![allow(dead_code, unused_imports, unused_variables)]

use rdd::get;

#[get("/x")]
async fn suelto() -> rdd::Result<u32> {
    Ok(1)
}

fn main() {}
