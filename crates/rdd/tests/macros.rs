//! Pruebas de las macros (§8) con `trybuild`.
//!
//! `ok/` compila **y se ejecuta**: verifica el comportamiento de las rutas
//! generadas. `fail/` fija el diagnóstico exacto de cada código del catálogo
//! §8.7; los `.stderr` se regeneran con `TRYBUILD=overwrite cargo test`.

#[test]
fn macros_ok() {
    let cases = trybuild::TestCases::new();
    cases.pass("tests/macros/ok/controller.rs");
    // §10.9: la fixture de GraphQL solo compila con la feature activa.
    #[cfg(feature = "graphql")]
    cases.pass("tests/macros/ok/graphql.rs");
}

#[test]
fn macros_fail() {
    let cases = trybuild::TestCases::new();
    cases.compile_fail("tests/macros/fail/rdd03*.rs");
    cases.compile_fail("tests/macros/fail/rdd04*.rs");
    #[cfg(feature = "graphql")]
    cases.compile_fail("tests/macros/fail/rdd050*.rs");
}
