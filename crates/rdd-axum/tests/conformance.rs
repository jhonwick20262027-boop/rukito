//! Conformidad de la plataforma Axum (§4.5, RP-020 a RP-022).
//!
//! Los casos viven en `rdd-platform-tests`; aquí solo se elige la plataforma.
//! Una plataforma nueva debe añadir su propio test de una línea como este.

#[tokio::test]
async fn axum_pasa_la_suite_de_conformidad() {
    rdd_platform_tests::run_all(rdd_axum::AxumPlatform::new).await;
}
