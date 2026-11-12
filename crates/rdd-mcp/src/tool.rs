//! Contrato de una tool MCP (§10.6, RF-208, RF-209).
//!
//! Una tool es un adaptador de entrada: recibe `arguments` ya decodificados,
//! llama al caso de uso del módulo y devuelve contenido. El protocolo no conoce
//! el dominio y la tool no conoce el transporte (ADR-22).

use anyhow::anyhow;
use rdd_core::Error;
use rdd_core::http::{BoxFuture, Extensions, FromContainer};
use serde::Serialize;
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// Tool expuesta por `tools/list` y ejecutable con `tools/call`.
pub trait Tool: Send + Sync + 'static {
    /// Nombre único con `[A-Za-z0-9_-]{1,64}` (RF-208).
    fn name(&self) -> &str;
    /// Descripción legible; puede ir vacía.
    fn description(&self) -> &str;
    /// JSON Schema del objeto `arguments`; debe declarar `"type": "object"`.
    fn input_schema(&self) -> Value;

    /// Ejecuta la tool. El resultado se publica tal cual en `tools/call`.
    fn call(
        &self,
        args: Value,
        ctx: ToolContext,
    ) -> BoxFuture<'static, rdd_core::Result<CallToolResult>>;
}

/// Contexto de una `tools/call` en vuelo (RF-211, RF-212).
///
/// Se entrega por valor; el token de cancelación y el canal de progreso son
/// clones baratos.
pub struct ToolContext {
    progress_token: Option<Value>,
    cancel: CancellationToken,
    out: mpsc::UnboundedSender<Value>,
    extensions: Extensions,
}

impl ToolContext {
    pub(crate) fn new(
        progress_token: Option<Value>,
        cancel: CancellationToken,
        out: mpsc::UnboundedSender<Value>,
        extensions: Extensions,
    ) -> Self {
        Self {
            progress_token,
            cancel,
            out,
            extensions,
        }
    }

    /// Emite `notifications/progress` si la petición trajo `_meta.progressToken`
    /// (RF-211). Sin token es un no-op exitoso.
    pub fn progress(&self, advance: f64, total: Option<f64>) -> rdd_core::Result<()> {
        let Some(token) = &self.progress_token else {
            return Ok(());
        };
        let mut params = json!({ "progressToken": token, "progress": advance });
        if let Some(total) = total {
            params["total"] = json!(total);
        }
        self.out
            .send(crate::jsonrpc::notification(
                "notifications/progress",
                params,
            ))
            .map_err(|_| Error::internal(anyhow!("el canal de salida se cerró")))?;
        Ok(())
    }

    /// `true` cuando el cliente envió `notifications/cancelled` para esta
    /// petición (RF-212).
    pub fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }

    /// Completa al cancelarse; se usa con `tokio::select!`.
    pub fn cancelled(&self) -> impl Future<Output = ()> + Send + '_ {
        self.cancel.cancelled()
    }

    /// Valor que dejó el transporte en las extensiones de la petición (RF-228).
    ///
    /// Por stdio son las que sembró `serve_stdio_with`; por HTTP, las que dejó
    /// la cadena global de `HttpApp::prepare` (por ejemplo, la `Session` que
    /// insertó el middleware JWT).
    pub fn ext<T: Clone + Send + Sync + 'static>(&self) -> Option<T> {
        self.extensions.get::<T>().cloned()
    }

    /// Servicio del contenedor que dejó la petición (RF-084, RF-228).
    ///
    /// Es el mismo mecanismo que `Request::resolve`: la tool no guarda servicios
    /// ni un dueño fijo, así que un `McpServer` sirve a todos los usuarios y a
    /// los dos transportes (ADR-25).
    pub fn resolve<T: FromContainer>(&self) -> rdd_core::Result<T> {
        let container = self
            .extensions
            .get::<std::sync::Arc<rdd_core::di::Container>>()
            .cloned()
            .unwrap_or_else(|| std::sync::Arc::clone(rdd_core::di::default_container()));
        T::from_container(&container)
    }
}

/// Resultado de una tool (RF-209).
#[derive(Debug, Clone, Serialize)]
pub struct CallToolResult {
    /// Contenido que el cliente muestra al modelo.
    pub content: Vec<Content>,
    /// `true` cuando el fallo lo produjo la tool y no el protocolo.
    #[serde(rename = "isError")]
    pub is_error: bool,
}

/// Bloque de contenido. En 0.7 solo existe texto (§2.2).
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Content {
    /// Texto plano.
    Text {
        /// Cuerpo del bloque.
        text: String,
    },
}

impl CallToolResult {
    /// Contenido de texto con `isError: false`.
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            content: vec![Content::Text { text: text.into() }],
            is_error: false,
        }
    }

    /// Serializa `value` como texto compacto (JSON aplanado).
    pub fn json(value: Value) -> Self {
        Self::text(value.to_string())
    }

    /// Fallo de la tool: el cliente ve `isError: true` y el mensaje.
    pub fn error(text: impl Into<String>) -> Self {
        Self {
            content: vec![Content::Text { text: text.into() }],
            is_error: true,
        }
    }
}
