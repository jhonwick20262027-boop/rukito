//! Pruebas de las macros (§8) con `trybuild`.
//!
//! `ok/` compila **y se ejecuta**: verifica el comportamiento de las rutas
//! generadas. `fail/` fija el diagnóstico exacto de cada código del catálogo
//! §8.7; los `.stderr` se regeneran con `TRYBUILD=overwrite cargo test`.

#[test]
fn macros_ok() {
    trybuild::TestCases::new().pass("tests/macros/ok/*.rs");
}

#[test]
fn macros_fail() {
    trybuild::TestCases::new().compile_fail("tests/macros/fail/*.rs");
}
