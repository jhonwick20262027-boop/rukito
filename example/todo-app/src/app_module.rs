//! Módulo raíz de la app (≈ app.module.ts, §6.2).
//!
//! Aquí van los componentes transversales. Los módulos de dominio se montan en
//! `main.rs`, debajo de `// rdd:modules` (§14.3).

use rdd::module;

use crate::app_controller::AppController;

/// El módulo raíz listo para montar.
pub fn module() -> rdd::Module {
    module! {
        providers: [],
        controllers: [AppController],
        middlewares: [],
    }
}
