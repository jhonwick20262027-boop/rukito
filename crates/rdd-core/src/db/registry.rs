//! Registro de adaptadores (§12.6).
//!
//! El núcleo no puede saber qué crate implementa `mongodb://` sin violar RA-014,
//! así que la fachada puebla este registro al arrancar — el mismo mecanismo que
//! RP-012 usa para la plataforma por defecto. Añadir un motor nuevo no toca este
//! archivo (RF-185).

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use crate::db::DbAdapter;
use crate::error::{Error, Result};
use crate::http::BoxFuture;

/// Factoría de un motor: recibe la URL ya resuelta y devuelve el adaptador conectado.
pub type AdapterFactory = fn(String) -> BoxFuture<'static, Result<Arc<dyn DbAdapter>>>;

/// Un motor conocido por la fachada, esté instalado o no.
struct Engine {
    crate_name: &'static str,
    feature: &'static str,
    factory: Option<AdapterFactory>,
}

static ENGINES: OnceLock<Mutex<HashMap<String, Engine>>> = OnceLock::new();

fn engines() -> MutexGuard<'static, HashMap<String, Engine>> {
    ENGINES
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|err| err.into_inner())
}

/// Declara un motor aunque su feature esté apagada: es lo que permite nombrar el
/// crate que falta en el error de RF-111. Idempotente; conserva una factoría ya
/// instalada para el mismo esquema.
pub fn declare(schemes: &[&'static str], crate_name: &'static str, feature: &'static str) {
    let mut map = engines();
    for scheme in schemes {
        let factory = map.get(*scheme).and_then(|engine| engine.factory);
        map.insert(
            (*scheme).to_owned(),
            Engine {
                crate_name,
                feature,
                factory,
            },
        );
    }
}

/// Instala la factoría de un motor compilado en la fachada.
pub fn install(scheme: &str, factory: AdapterFactory) {
    let mut map = engines();
    match map.get_mut(scheme) {
        Some(engine) => engine.factory = Some(factory),
        None => {
            map.insert(
                scheme.to_owned(),
                Engine {
                    crate_name: "",
                    feature: "",
                    factory: Some(factory),
                },
            );
        }
    }
}

/// Esquema de una URL, o cadena vacía si no trae `://`.
pub fn scheme_of(url: &str) -> &str {
    url.split_once("://")
        .map(|(scheme, _)| scheme)
        .unwrap_or("")
}

/// Esquemas declarados, ordenados, para mensajes y `rdd doctor`.
pub fn declared() -> Vec<String> {
    let mut schemes: Vec<String> = engines().keys().cloned().collect();
    schemes.sort();
    schemes
}

/// Resuelve la factoría de la URL, o falla al arrancar diciendo qué añadir (RF-111).
pub fn factory_for(url: &str) -> Result<AdapterFactory> {
    let scheme = scheme_of(url);
    // Se copia fuera del guard: `declared()` vuelve a tomar el mutex y un
    // `std::sync::Mutex` no es reentrante.
    let engine = engines()
        .get(scheme)
        .map(|engine| (engine.crate_name, engine.feature, engine.factory));
    match engine {
        Some((_, _, Some(factory))) => Ok(factory),
        Some((crate_name, feature, None)) => Err(Error::internal(anyhow::anyhow!(
            "no hay adaptador instalado para `{scheme}://`.\n  \
             añade `{crate_name}` (feature `{feature}` de `rdd`), o registra el tuyo con \
             `rdd::db::install(...)` (§12.6, RF-111)"
        ))),
        None => Err(Error::internal(anyhow::anyhow!(
            "esquema de base de datos no soportado: `{scheme}`.\n  \
             declarados: {}\n  \
             usa `.in_memory()` para desarrollo y tests (RF-015, RF-111)",
            declared().join(", ")
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::MemoryBackend;

    fn fake(_url: String) -> BoxFuture<'static, Result<Arc<dyn DbAdapter>>> {
        Box::pin(async { Ok(Arc::new(MemoryBackend::new()) as Arc<dyn DbAdapter>) })
    }

    #[test]
    fn extrae_el_esquema() {
        assert_eq!(scheme_of("postgres://u:p@h/db"), "postgres");
        assert_eq!(scheme_of("postgresql://h/db"), "postgresql");
        assert_eq!(scheme_of("sin-esquema"), "");
    }

    #[test]
    fn un_esquema_declarado_sin_instalar_nombra_crate_y_feature() {
        declare(&["rdd-test-declarado"], "rdd-fantasma", "fantasma");
        let err = factory_for("rdd-test-declarado://x").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("rdd-fantasma"), "{msg}");
        assert!(msg.contains("fantasma"), "{msg}");
    }

    #[test]
    fn instalar_resuelve_la_factoria() {
        declare(&["rdd-test-instalado"], "rdd-falso", "falso");
        install("rdd-test-instalado", fake);
        assert!(factory_for("rdd-test-instalado://x").is_ok());
    }

    #[test]
    fn declarar_no_pisa_una_factoria_instalada() {
        install("rdd-test-orden", fake);
        declare(&["rdd-test-orden"], "rdd-falso", "falso");
        assert!(factory_for("rdd-test-orden://x").is_ok());
    }

    #[test]
    fn un_esquema_desconocido_lista_los_declarados() {
        declare(&["rdd-test-conocido"], "rdd-falso", "falso");
        let err = factory_for("rdd-test-inventado://x").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("rdd-test-inventado"), "{msg}");
        assert!(msg.contains("rdd-test-conocido"), "{msg}");
        assert!(msg.contains("in_memory"), "{msg}");
    }
}
