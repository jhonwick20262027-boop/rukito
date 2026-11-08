//! Conformidad de la plataforma de referencia del núcleo: `HttpApp::dispatch`
//! (§4.5, RP-014, RP-021).
//!
//! Corre los mismos casos que cualquier plataforma, sin socket. Si la sonda de
//! red y esta divergen, el fallo está en el núcleo o en el adaptador, nunca en
//! la suite.

#[tokio::test]
async fn dispatch_pasa_la_suite_de_conformidad() {
    rdd_platform_tests::run_all_dispatch().await;
}
