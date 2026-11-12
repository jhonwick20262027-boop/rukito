//! `require_session`: guardián de entrada de `/mcp` (§26.8, RF-231).
//!
//! No es un `Middleware`: corre después de la cadena global y antes de tocar el
//! protocolo (RP-029). Ve la `Session` que dejó `JwtMiddleware`, así que no repite
//! CORS ni el rate limit.

use rdd::prelude::*;

use crate::modules::users::Session;

pub fn require_session(req: &rdd::http::Request) -> Result<()> {
    req.ext::<Session>().map(|_| ()).ok_or(Error::Unauthorized)
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use crate::modules::users::Role;

    use super::*;

    fn request(session: Option<Session>) -> rdd::http::Request {
        let mut req = http::Request::builder()
            .method(rdd::http::Method::POST)
            .uri("/mcp")
            .body(bytes::Bytes::new())
            .unwrap();
        if let Some(session) = session {
            req.extensions_mut().insert(session);
        }
        req
    }

    #[test]
    fn sin_sesion_es_401() {
        let err = require_session(&request(None)).unwrap_err();
        assert_eq!(err.status(), StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn con_sesion_deja_pasar() {
        let session = Session {
            id: Uuid::nil(),
            email: "ana@correo.com".to_owned(),
            role: Role::User,
        };
        assert!(require_session(&request(Some(session))).is_ok());
    }
}
