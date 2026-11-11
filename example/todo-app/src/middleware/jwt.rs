//! `JwtMiddleware`: identifica, no rechaza (§26.3).
//!
//! Es global y corre antes del `RateLimit`, que necesita la identidad para su
//! clave (RF-109). Si rechazara aquí, dejaría fuera `/auth/register`,
//! `/auth/login` y `/health`; quien exige sesión es `RequireAuth`, por
//! controller, porque RF-080 pone los globales antes que todo lo demás.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use rdd::http::NextArc;
use rdd::prelude::*;

use crate::modules::users::TokenCodec;

pub struct JwtMiddleware {
    tokens: Arc<dyn TokenCodec>,
}

impl JwtMiddleware {
    pub fn new(tokens: Arc<dyn TokenCodec>) -> Self {
        Self { tokens }
    }
}

impl Middleware for JwtMiddleware {
    fn handle<'a>(
        &'a self,
        req: Request,
        next: NextArc,
    ) -> Pin<Box<dyn Future<Output = Result<Response>> + Send + 'a>> {
        Box::pin(async move {
            let mut req = req;
            if let Some(token) = bearer(&req) {
                match self.tokens.verify(token) {
                    Ok(Some(session)) => {
                        req.extensions_mut().insert(session);
                    }
                    Ok(None) => {
                        tracing::debug!("token inválido o expirado: la petición sigue sin sesión");
                    }
                    Err(err) => {
                        tracing::warn!(error = %err, "no se pudo verificar el token");
                    }
                }
            }
            next(req).await
        })
    }
}

/// `Authorization: Bearer <token>`. El esquema no distingue mayúsculas (RFC
/// 7235); un valor que no sea texto UTF-8 se trata como ausencia de token.
fn bearer(req: &Request) -> Option<&str> {
    let value = req.headers().get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    let token = token.trim();
    if scheme.eq_ignore_ascii_case("Bearer") && !token.is_empty() {
        Some(token)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use crate::modules::users::{Role, Session};

    use super::*;

    /// Un codec de mentira: el middleware se prueba contra el puerto, no contra
    /// `jsonwebtoken`. Así el test no depende del secreto del entorno.
    struct FakeCodec;

    impl TokenCodec for FakeCodec {
        fn issue(&self, _session: &Session) -> Result<String> {
            Ok("token-falso".to_owned())
        }

        fn verify(&self, token: &str) -> Result<Option<Session>> {
            Ok((token == "token-falso").then(|| Session {
                id: Uuid::nil(),
                email: "ana@correo.com".to_owned(),
                role: Role::User,
            }))
        }
    }

    /// Devuelve en el cuerpo lo que vio en las extensiones: es la forma de
    /// comprobar que la identidad llegó hasta el handler.
    fn echo_session() -> NextArc {
        Arc::new(|req: Request| {
            Box::pin(async move {
                let body = match req.ext::<Session>() {
                    Some(session) => session.email,
                    None => "sin-sesion".to_owned(),
                };
                Ok(Response::new(bytes::Bytes::from(body)))
            })
        })
    }

    fn request(authorization: Option<&str>) -> Request {
        let mut builder = http::Request::builder()
            .method(rdd::http::Method::GET)
            .uri("/tasks");
        if let Some(value) = authorization {
            builder = builder.header(header::AUTHORIZATION, value);
        }
        builder.body(bytes::Bytes::new()).unwrap()
    }

    async fn body(authorization: Option<&str>) -> String {
        let middleware = JwtMiddleware::new(Arc::new(FakeCodec));
        let resp = middleware
            .handle(request(authorization), echo_session())
            .await
            .unwrap();
        String::from_utf8(resp.body().to_vec()).unwrap()
    }

    #[tokio::test]
    async fn un_token_valido_deja_la_sesion_en_las_extensiones() {
        assert_eq!(body(Some("Bearer token-falso")).await, "ana@correo.com");
        assert_eq!(body(Some("bearer token-falso")).await, "ana@correo.com");
    }

    /// §26.3: sin cabecera, con un token inválido o con otro esquema, la cadena
    /// sigue. Rechazar es trabajo de `RequireAuth`.
    #[tokio::test]
    async fn sin_identidad_valida_la_peticion_pasa_igual() {
        for authorization in [
            None,
            Some("Bearer token-malo"),
            Some("Basic token-falso"),
            Some("Bearer"),
            Some("Bearer   "),
            Some("token-falso"),
        ] {
            assert_eq!(body(authorization).await, "sin-sesion", "{authorization:?}");
        }
    }
}
