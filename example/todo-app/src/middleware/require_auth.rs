//! `RequireAuth`: 401 si no hay sesión (§26.3).
//!
//! Se monta por controller, no global: `/auth/register`, `/auth/login` y
//! `/health` tienen que seguir siendo públicos (§26.4). La identidad la puso
//! antes `JwtMiddleware`, que es global porque solo identifica.

use std::future::Future;
use std::pin::Pin;

use rdd::http::NextArc;
use rdd::prelude::*;

use crate::modules::users::Session;

pub struct RequireAuth;

impl Middleware for RequireAuth {
    fn handle<'a>(
        &'a self,
        req: Request,
        next: NextArc,
    ) -> Pin<Box<dyn Future<Output = Result<Response>> + Send + 'a>> {
        Box::pin(async move {
            // RF-082: corta sin llamar a `next`. El núcleo materializa el 401 en
            // Problem Details (§10.3); aquí no se construye ninguna respuesta.
            if req.ext::<Session>().is_none() {
                return Err(Error::Unauthorized);
            }
            next(req).await
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use uuid::Uuid;

    use crate::modules::users::Role;

    use super::*;

    fn next() -> NextArc {
        Arc::new(|_req| Box::pin(async { Ok(Response::new(bytes::Bytes::from_static(b"next"))) }))
    }

    fn request(session: Option<Session>) -> Request {
        let mut req = http::Request::builder()
            .method(rdd::http::Method::GET)
            .uri("/tasks")
            .body(bytes::Bytes::new())
            .unwrap();
        if let Some(session) = session {
            req.extensions_mut().insert(session);
        }
        req
    }

    fn session() -> Session {
        Session {
            id: Uuid::nil(),
            email: "ana@correo.com".to_owned(),
            role: Role::User,
        }
    }

    #[tokio::test]
    async fn sin_sesion_es_401_y_no_llama_a_next() {
        let err = RequireAuth.handle(request(None), next()).await.unwrap_err();
        assert_eq!(err.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn con_sesion_deja_pasar() {
        let resp = RequireAuth
            .handle(request(Some(session())), next())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(resp.body().as_ref(), b"next");
    }
}
