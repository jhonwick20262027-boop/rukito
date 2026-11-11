//! Lectura de la sección `[cors]` de `rdd.toml` (RF-093, RF-151).
//!
//! Este crate lee **solo su sección**: el resto del archivo lo leen el CLI y los
//! demás paquetes, así que las secciones ajenas se ignoran en vez de rechazarse.

use std::path::Path;
use std::time::Duration;

use serde::Deserialize;

use crate::{Cors, CorsBuilder, CorsConfigError};

/// El archivo completo. A propósito **sin** `deny_unknown_fields`: `rdd.toml`
/// lleva `[server]`, `[database]` y `[logging]`, que no son asunto de este crate.
#[derive(Debug, Deserialize)]
struct File {
    cors: Option<Section>,
}

/// La sección `[cors]` de §15.1. Aquí sí se rechazan claves desconocidas: un
/// `allow_origin` en singular escrito por error dejaría el CORS cerrado en
/// silencio, y eso es una decisión de seguridad que no puede fallar callada.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Section {
    #[serde(default)]
    allow_origins: Vec<String>,
    #[serde(default)]
    allow_methods: Vec<String>,
    #[serde(default)]
    allow_headers: Vec<String>,
    #[serde(default)]
    expose_headers: Vec<String>,
    #[serde(default)]
    allow_credentials: bool,
    max_age_secs: Option<u64>,
}

/// Lee el archivo. Que no exista se trata igual que no tener sección `[cors]`:
/// denegar todo (RF-090), nunca abrir por omisión.
pub(super) fn builder(path: &Path) -> Result<CorsBuilder, CorsConfigError> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Cors::new()),
        Err(err) => {
            return Err(CorsConfigError::Config(format!(
                "{}: {err}",
                path.display()
            )));
        }
    };
    parse(&text)
}

pub(super) fn parse(text: &str) -> Result<CorsBuilder, CorsConfigError> {
    let file: File =
        toml::from_str(text).map_err(|err| CorsConfigError::Config(err.to_string()))?;
    let Some(section) = file.cors else {
        return Ok(Cors::new());
    };

    let mut builder = Cors::new().allow_credentials(section.allow_credentials);
    for origin in section.allow_origins {
        builder = builder.allow_origin(origin);
    }
    builder = builder
        .allow_methods(section.allow_methods)
        .allow_headers(section.allow_headers)
        .expose_headers(section.expose_headers);
    if let Some(secs) = section.max_age_secs {
        builder = builder.max_age(Duration::from_secs(secs));
    }
    Ok(builder)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL: &str = r#"
[server]
addr = "127.0.0.1:3000"

[cors]
allow_origins     = ["https://miapp.com", "http://localhost:5173"]
allow_methods     = ["GET", "POST", "PUT", "DELETE"]
allow_headers     = ["content-type", "authorization"]
expose_headers    = ["x-request-id", "ratelimit-remaining"]
allow_credentials = true
max_age_secs      = 3600
"#;

    /// RF-093: las seis claves llegan al middleware, y las secciones ajenas no
    /// estorban (RF-151).
    #[test]
    fn lee_la_seccion_completa() {
        let cors = parse(FULL).unwrap().build().unwrap();
        assert_eq!(
            cors.origins,
            vec![
                "https://miapp.com".to_owned(),
                "http://localhost:5173".to_owned()
            ]
        );
        assert!(cors.credentials);
        assert_eq!(cors.allow_methods, "GET, POST, PUT, DELETE");
        assert_eq!(cors.allow_headers, "content-type, authorization");
        assert_eq!(
            cors.expose_headers.unwrap(),
            "x-request-id, ratelimit-remaining"
        );
        assert_eq!(cors.max_age.unwrap(), "3600");
    }

    /// RF-090/RF-093: sin sección, denegar todo.
    #[test]
    fn sin_seccion_deniega_todo() {
        for text in ["", "[server]\naddr = \"127.0.0.1:3000\"\n"] {
            let cors = parse(text).unwrap().build().unwrap();
            assert!(cors.origins.is_empty(), "{text}");
            assert!(!cors.wildcard);
            assert!(!cors.credentials);
        }
    }

    #[test]
    fn una_seccion_vacia_tambien_deniega_todo() {
        let cors = parse("[cors]\n").unwrap().build().unwrap();
        assert!(cors.origins.is_empty());
        assert!(!cors.wildcard);
    }

    /// RF-092: la combinación prohibida se rechaza también viniendo del archivo.
    #[test]
    fn rechaza_comodin_con_credenciales_desde_el_archivo() {
        let text = "[cors]\nallow_origins = [\"*\"]\nallow_credentials = true\n";
        assert_eq!(
            parse(text).unwrap().build().unwrap_err(),
            CorsConfigError::CredentialsWithWildcard
        );
    }

    /// El error nombra el valor inválido, no solo la clave.
    #[test]
    fn nombra_el_metodo_y_la_cabecera_invalidos() {
        let text = "[cors]\nallow_methods = [\"NO ES UN MÉTODO\"]\n";
        let error = parse(text).unwrap().build().unwrap_err();
        assert!(error.to_string().contains("NO ES UN MÉTODO"), "{error}");

        let text = "[cors]\nallow_headers = [\"cabecera inválida\"]\n";
        let error = parse(text).unwrap().build().unwrap_err();
        assert!(error.to_string().contains("cabecera inválida"), "{error}");
    }

    /// Un `allow_origin` en singular es un error, no un CORS cerrado en silencio.
    #[test]
    fn rechaza_claves_desconocidas_en_la_seccion() {
        let text = "[cors]\nallow_origin = [\"https://miapp.com\"]\n";
        let error = parse(text).unwrap_err();
        assert!(matches!(error, CorsConfigError::Config(_)), "{error}");
        assert!(error.to_string().contains("allow_origin"), "{error}");
    }

    #[test]
    fn un_toml_roto_es_error_no_configuracion_vacia() {
        let error = parse("[cors\nallow_origins = ").unwrap_err();
        assert!(matches!(error, CorsConfigError::Config(_)), "{error}");
    }

    /// Sin archivo el comportamiento es el de RF-090, no un fallo de arranque.
    #[test]
    fn un_archivo_inexistente_deniega_todo() {
        let path = std::env::temp_dir().join("rdd-cors-no-existe-jamas.toml");
        let _ = std::fs::remove_file(&path);
        let cors = builder(&path).unwrap().build().unwrap();
        assert!(cors.origins.is_empty());
        assert!(!cors.wildcard);
    }
}
