//! RDD0302: los handlers son funciones asociadas, no métodos con `self` (§8.7).

#![allow(dead_code, unused_imports, unused_variables)]

use rdd::{controller, get};

pub struct C;

#[controller("/c")]
impl C {
    #[get("/")]
    async fn list(&self) -> rdd::Result<u32> {
        Ok(1)
    }
}

fn main() {}
