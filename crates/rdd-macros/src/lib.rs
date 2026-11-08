//! Macros del framework RDD (§8).
//!
//! Ninguna usa `unsafe` (RN-011), no requiere nightly (RN-010) y todo lo que
//! genera va con rutas absolutas `::rdd::...` para no colisionar con nombres del
//! usuario (RN-012). El crate **no** depende de `rdd-core` (RA-010): el contrato
//! entre macro y núcleo son las funciones públicas de [`rdd::api`] y
//! [`rdd::http`](rdd::http).

use proc_macro::TokenStream;

mod controller;
mod module;

/// Declara un controller HTTP sobre un `impl` (§8.2).
///
/// El prefijo va entre comillas: `#[controller("/tasks")]`. Cada método del
/// bloque lleva un atributo de verbo; la macro genera los `RouteDef` (RF-056),
/// la función `routes()` y el registro del módulo.
#[proc_macro_attribute]
pub fn controller(args: TokenStream, item: TokenStream) -> TokenStream {
    controller::expand_controller(args.into(), item.into()).into()
}

/// Middlewares de controller o de ruta (§11.4). Dentro de `#[controller]` los
/// consume la macro; usados sueltos son un error de programa.
#[proc_macro_attribute]
pub fn middleware(args: TokenStream, item: TokenStream) -> TokenStream {
    controller::standalone_middleware(args.into(), item.into()).into()
}

/// `#[rdd::main]`: runtime Tokio, `.env`, `tracing` y código de salida (§8.4).
#[proc_macro_attribute]
pub fn main(args: TokenStream, item: TokenStream) -> TokenStream {
    controller::expand_main(args.into(), item.into()).into()
}

/// Construye un `Module` (§6.2), con claves o en forma abreviada.
#[proc_macro]
pub fn module(args: TokenStream) -> TokenStream {
    module::expand(args.into()).into()
}

macro_rules! verbo {
    ($nombre:ident, $metodo:literal) => {
        #[doc = concat!(
            "Ruta `", $metodo, "` dentro de un `impl` marcado con `#[controller]` (§8.2).\n\n",
            "Fuera de un controller es un error de programa: la macro del verbo solo ",
            "tiene sentido como parte de la declaración de una ruta."
        )]
        #[proc_macro_attribute]
        pub fn $nombre(_args: TokenStream, item: TokenStream) -> TokenStream {
            controller::standalone_verb($metodo, stringify!($nombre), item.into()).into()
        }
    };
}

verbo!(get, "GET");
verbo!(post, "POST");
verbo!(put, "PUT");
verbo!(patch, "PATCH");
verbo!(delete, "DELETE");
