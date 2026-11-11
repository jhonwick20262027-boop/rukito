//! `RequireRole`: 403 si el rol no alcanza (§26.3).
//!
//! Se monta por ruta con `#[middleware(RequireRole::new(Role::Admin))]` (§11.4,
//! RF-086). Corre después de `RequireAuth`, pero no depende de él: si no hay
//! sesión responde 401, no 403, porque «no autenticado» y «sin permiso» son
//! estados distintos (§10.3).

use std::future::Future;
use std::pin::Pin;

use rdd::http::NextArc;
use rdd::prelude::*;

use crate::modules::users::{Role, Session};

pub struct RequireRole {
    required: Role,
}

impl RequireRole {
    pub fn new(required: Role) -> Self {
        Self { required }
    }
}

impl Middleware for RequireRole {
    fn handle<'a>(
        &'a self,
        req: Request,
        next: NextArc,
    ) -> Pin<Box<dyn Future<Output = Result<Response>> + Send + 'a>> {
        Box::pin(async move {
            let Some(session) = req.ext::<Session>() else {
                return Err(Error::Unauthorized);
            };
            // RF-082: corta sin llamar a `next`; el núcleo materializa el 403.
            if !session.role.satisfies(self.required) {
                return Err(Error::Forbidden);
            }
            next(req).await
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use uuid::Uuid;

    use super::*;

    fn next() -> NextArc {
        Arc::new(|_req| Box::pin(async { Ok(Response::new(bytes::Bytes::from_static(b"next"))) }))
    }

    fn request(role: Option<Role>) -> Request {
        let mut req = http::Request::builder()
            .method(rdd::http::Method::DELETE)
            .uri("/tasks/1")
            .body(bytes::Bytes::new())
            .unwrap();
        if let Some(role) = role {
            req.extensions_mut().insert(Session {
                id: Uuid::nil(),
                email: "ana@correo.com".to_owned(),
                role,
            });
        }
        req
    }

    #[tokio::test]
    async fn un_user_no_alcanza_lo_que_exige_admin() {
        let err = RequireRole::new(Role::Admin)
            .handle(request(Some(Role::User)), next())
            .await
            .unwrap_err();
        assert_eq!(err.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn admin_alcanza_cualquier_ruta() {
        for required in [Role::User, Role::Admin] {
            let resp = RequireRole::new(required)
                .handle(request(Some(Role::Admin)), next())
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
        }
    }

    #[tokio::test]
    async fn user_pasa_lo_que_exige_user() {
        let resp = RequireRole::new(Role::User)
            .handle(request(Some(Role::User)), next())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn sin_sesion_es_401_no_403() {
        let err = RequireRole::new(Role::Admin)
            .handle(request(None), next())
            .await
            .unwrap_err();
        assert_eq!(err.status(), StatusCode::UNAUTHORIZED);
    }
}
