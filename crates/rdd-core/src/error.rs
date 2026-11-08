//! Errores del dominio y del framework, y su conversión a Problem Details (RFC 9457).

use std::fmt;

/// Alias de resultado del framework: el error por defecto es [`Error`].
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Resultado de nivel de arranque/plataforma, donde sí importa el error original.
pub type PlatformResult<T> = std::result::Result<T, PlatformError>;

/// Error único que recorre el dominio, los servicios y la capa HTTP.
///
/// El dominio devuelve estos valores sin conocer HTTP; la conversión a estado
/// HTTP ocurre una sola vez, en el núcleo (§10.3).
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Petición malformada → 400.
    #[error("bad request: {0}")]
    BadRequest(String),
    /// Regla de negocio o validación de dominio → 422.
    #[error("invalid: {0}")]
    Invalid(String),
    /// Credenciales ausentes o inválidas → 401.
    #[error("unauthorized")]
    Unauthorized,
    /// Autenticado pero sin permiso → 403.
    #[error("forbidden")]
    Forbidden,
    /// Recurso inexistente → 404.
    #[error("not found: {0}")]
    NotFound(String),
    /// Conflicto de estado o de unicidad → 409.
    #[error("conflict: {0}")]
    Conflict(String),
    /// Fallo interno. Su detalle **no** se envía al cliente (RF-041).
    #[error("internal error: {0}")]
    Internal(anyhow::Error),
}

impl Error {
    pub fn bad_request(msg: impl Into<String>) -> Self {
        Self::BadRequest(msg.into())
    }
    pub fn invalid(msg: impl Into<String>) -> Self {
        Self::Invalid(msg.into())
    }
    pub fn not_found(msg: impl Into<String>) -> Self {
        Self::NotFound(msg.into())
    }
    pub fn conflict(msg: impl Into<String>) -> Self {
        Self::Conflict(msg.into())
    }
    pub fn internal(err: anyhow::Error) -> Self {
        Self::Internal(err)
    }

    /// Estado HTTP asociado (§10.3).
    pub fn status(&self) -> http::StatusCode {
        use http::StatusCode as S;
        match self {
            Self::BadRequest(_) => S::BAD_REQUEST,
            Self::Unauthorized => S::UNAUTHORIZED,
            Self::Forbidden => S::FORBIDDEN,
            Self::NotFound(_) => S::NOT_FOUND,
            Self::Conflict(_) => S::CONFLICT,
            Self::Invalid(_) => S::UNPROCESSABLE_ENTITY,
            Self::Internal(_) => S::INTERNAL_SERVER_ERROR,
        }
    }

    /// `title` de Problem Details: el motivo HTTP canónico del estado.
    pub fn title(&self) -> &'static str {
        self.status()
            .canonical_reason()
            .unwrap_or("Internal Server Error")
    }

    /// Texto para `detail`. Para [`Error::Internal`] devuelve un texto genérico:
    /// el detalle real solo va al log (RF-041).
    pub fn client_detail(&self) -> String {
        match self {
            Self::BadRequest(m) | Self::Invalid(m) | Self::NotFound(m) | Self::Conflict(m) => {
                m.clone()
            }
            Self::Unauthorized => "se requieren credenciales".to_owned(),
            Self::Forbidden => "no hay permiso para esta operación".to_owned(),
            Self::Internal(_) => "ha ocurrido un error interno".to_owned(),
        }
    }

    /// Es un error de infraestructura: debe registrarse con `tracing::error!`.
    pub fn is_internal(&self) -> bool {
        matches!(self, Self::Internal(_))
    }
}

/// Fallo del arranque, de la plataforma o del entorno, distinto de un error de petición.
#[derive(Debug, thiserror::Error)]
pub enum PlatformError {
    /// No se pudo abrir el socket de escucha.
    #[error("no se pudo escuchar en {addr}: {source}")]
    Bind {
        addr: String,
        #[source]
        source: std::io::Error,
    },
    /// Fallo del servidor interno de la plataforma.
    #[error("fallo de la plataforma {platform}: {message}")]
    Server {
        platform: &'static str,
        message: String,
    },
    /// Fallo al construir el contenedor de dependencias (§6.3).
    #[error(transparent)]
    Container(ContainerError),
    /// Fallo de configuración o de base de datos.
    #[error(transparent)]
    Other(anyhow::Error),
}

impl PlatformError {
    pub fn other(err: impl Into<anyhow::Error>) -> Self {
        Self::Other(err.into())
    }
}

impl From<ContainerError> for Error {
    fn from(err: ContainerError) -> Self {
        Self::Internal(anyhow::Error::new(err))
    }
}

impl From<PlatformError> for Error {
    fn from(err: PlatformError) -> Self {
        match err {
            PlatformError::Container(err) => Self::from(err),
            other => Self::Internal(anyhow::Error::new(other)),
        }
    }
}

impl From<ContainerError> for PlatformError {
    fn from(e: ContainerError) -> Self {
        Self::Container(e)
    }
}

/// Error del contenedor de dependencias, con el formato exigido por RF-024/RF-025.
#[derive(Debug, thiserror::Error)]
pub enum ContainerError {
    /// Falta una dependencia (RF-024).
    #[error(
        "falta la dependencia `{tipo}` solicitada por {solicitante}.{campo}\n  \
         para registrarla: \n  \
         .provide::<{tipo}>(...) en App, o declara un `#[service]` que la construya\n  \
         (ver RF-022: tipos inyectables = otros services, Repo<T>, Config<T>, Arc<T> registrados con .provide)"
    )]
    Missing {
        solicitante: String,
        campo: String,
        tipo: String,
    },
    /// Dependencia circular (RF-025). `ciclo` llega ya formateado: `A -> B -> A`.
    #[error("dependencia circular: {ciclo}")]
    Cycle { ciclo: String },
    /// Un provider falló al construirse.
    #[error("al construir el provider `{tipo}`: {source}")]
    Init {
        tipo: String,
        #[source]
        source: anyhow::Error,
    },
}

/// Error de dominio para campos que no son inyectables según RF-022.
#[derive(Debug)]
pub struct NotInjectable(pub &'static str);

impl fmt::Display for NotInjectable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "el tipo `{}` no es inyectable", self.0)
    }
}

impl std::error::Error for NotInjectable {}

/// Cuerpo de error en formato Problem Details (§10.3, ADR-08).
#[derive(Debug, Clone, serde::Serialize)]
pub struct Problem {
    pub r#type: String,
    pub title: String,
    pub status: u16,
    pub detail: String,
    pub instance: String,
}

impl Problem {
    /// Problema desde un [`Error`] y la ruta que lo provocó.
    pub fn from_error(err: &Error, instance: &str) -> Self {
        Self {
            r#type: "about:blank".to_owned(),
            title: err.title().to_owned(),
            status: err.status().as_u16(),
            detail: err.client_detail(),
            instance: instance.to_owned(),
        }
    }

    /// 413 sin leer el resto del cuerpo (RP-006).
    pub fn payload_too_large(limit: usize, instance: &str) -> Self {
        Self {
            r#type: "about:blank".to_owned(),
            title: "Payload Too Large".to_owned(),
            status: http::StatusCode::PAYLOAD_TOO_LARGE.as_u16(),
            detail: format!("el cuerpo supera el límite de {limit} bytes"),
            instance: instance.to_owned(),
        }
    }
}
