//! Endpoint validado de un adaptador (§10.13, RF-275).
//!
//! Solo `https` con raíces webpki, salvo un `http` que el operador escriba de
//! forma explícita para un emulador.

use crate::error::UploadConfigError;

/// Esquema, autoridad y prefijo de ruta de un endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Endpoint {
    scheme: String,
    host: String,
    base_path: String,
}

impl Endpoint {
    /// Valida y descompone `https://host[:puerto][/prefijo]`.
    pub(crate) fn parse(raw: &str) -> Result<Self, UploadConfigError> {
        let raw = raw.trim();
        let (scheme, rest) = raw.split_once("://").ok_or_else(|| {
            UploadConfigError::endpoint(raw, "falta el esquema `https://` o `http://`")
        })?;
        let scheme = scheme.to_ascii_lowercase();
        if scheme != "https" && scheme != "http" {
            return Err(UploadConfigError::endpoint(
                raw,
                "solo se admite `https` o `http` explícito para un emulador",
            ));
        }
        let (authority, path) = match rest.split_once('/') {
            Some((authority, path)) => (authority, format!("/{path}")),
            None => (rest, String::new()),
        };
        if authority.is_empty() {
            return Err(UploadConfigError::endpoint(raw, "falta el host"));
        }
        if authority.contains('@') {
            return Err(UploadConfigError::endpoint(
                raw,
                "no se admiten credenciales en la URL",
            ));
        }
        if authority.contains('?') || authority.contains('#') {
            return Err(UploadConfigError::endpoint(
                raw,
                "no se admiten query ni fragmento en el endpoint",
            ));
        }
        if path.contains('?') || path.contains('#') {
            return Err(UploadConfigError::endpoint(
                raw,
                "no se admiten query ni fragmento en el endpoint",
            ));
        }
        let base_path = path.trim_end_matches('/').to_owned();
        Ok(Self {
            scheme,
            host: authority.to_ascii_lowercase(),
            base_path,
        })
    }

    /// `https` o `http`.
    pub(crate) fn scheme(&self) -> &str {
        &self.scheme
    }

    /// Autoridad con puerto, en minúsculas.
    pub(crate) fn host(&self) -> &str {
        &self.host
    }

    /// Prefijo de ruta sin barra final; vacío si no hay.
    pub(crate) fn base_path(&self) -> &str {
        &self.base_path
    }

    /// `esquema://host`.
    pub(crate) fn origin(&self) -> String {
        format!("{}://{}", self.scheme, self.host)
    }

    /// URL absoluta para una ruta que empieza por `/`.
    pub(crate) fn url(&self, path: &str) -> String {
        format!("{}{}", self.origin(), path)
    }

    /// Endpoint público de S3 para una región (`us-east-1` incluido).
    pub(crate) fn aws_s3(region: &str) -> Result<Self, UploadConfigError> {
        Ok(Self {
            scheme: "https".to_owned(),
            host: format!("s3.{region}.amazonaws.com"),
            base_path: String::new(),
        })
    }
}

/// Interpreta un booleano de entorno (`true`, `1`, `yes`, `sí`).
pub(crate) fn parse_bool(name: &'static str, raw: &str) -> Result<bool, UploadConfigError> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "si" | "sí" => Ok(true),
        "false" | "0" | "no" => Ok(false),
        _ => Err(UploadConfigError::invalid(
            name,
            format!("se esperaba true o false, no `{raw}`"),
        )),
    }
}

/// Interpreta un entero positivo de entorno.
pub(crate) fn parse_usize(name: &'static str, raw: &str) -> Result<usize, UploadConfigError> {
    raw.trim()
        .parse()
        .map_err(|_| UploadConfigError::invalid(name, format!("se esperaba un número, no `{raw}`")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valida_esquema_y_host() {
        let endpoint = Endpoint::parse("https://storage.googleapis.com/").unwrap();
        assert_eq!(endpoint.origin(), "https://storage.googleapis.com");
        assert_eq!(endpoint.base_path(), "");

        let azurite = Endpoint::parse("http://127.0.0.1:10000/devstoreaccount1").unwrap();
        assert_eq!(azurite.host(), "127.0.0.1:10000");
        assert_eq!(azurite.base_path(), "/devstoreaccount1");

        for raw in ["ftp://host", "host", "https://", "https://user@host"] {
            assert!(Endpoint::parse(raw).is_err(), "aceptó {raw}");
        }
    }
}
