//! RDD0401: `#[middleware(…)]` suelto, fuera de un `#[controller]` (§11.4, §8.7).

#![allow(dead_code, unused_imports, unused_variables)]

use rdd::middleware;

#[middleware(Algo)]
struct S;

fn main() {}
