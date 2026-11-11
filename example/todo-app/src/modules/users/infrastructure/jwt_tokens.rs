//! Adaptador HS256 del puerto `TokenCodec` (§26.3, RA-007).
//!
//! El secreto llega del entorno por `config/jwt_config.rs`; este archivo no lee
//! variables ni ficheros. HS256 basta para un demo con un solo servicio: con
//! varios emisores harían falta claves asimétricas.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use rdd::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::modules::users::domain::{Role, Session, TokenCodec};

/// Un solo `Claims` para emitir y verificar (§26.3). `sub` es el subject
/// estándar del JWT; el rol viaja como texto para que `Role::parse` siga siendo
/// la única puerta de entrada a un valor desconocido.
#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    sub: Uuid,
    email: String,
    role: Role,
    exp: usize,
}

/// Clonable porque el contenedor lo guarda como singleton y lo reparte por `Arc`
/// a cada `UsersService` que se resuelve (§6.3).
#[derive(Clone)]
pub struct JwtTokens {
    encoding: EncodingKey,
    decoding: DecodingKey,
    ttl: Duration,
    validation: Validation,
}

impl JwtTokens {
    pub fn new(secret: &str, ttl: Duration) -> Self {
        // Fijar el algoritmo en la verificación, y no confiar en el del token:
        // sin esto un `alg: none` o un RS256 cambiado se aceptaría (RFC 8725).
        let mut validation = Validation::new(Algorithm::HS256);
        // Sin margen de cortesía: la expiración del demo es exacta y comprobable.
        validation.leeway = 0;
        Self {
            encoding: EncodingKey::from_secret(secret.as_bytes()),
            decoding: DecodingKey::from_secret(secret.as_bytes()),
            ttl,
            validation,
        }
    }
}

impl TokenCodec for JwtTokens {
    fn issue(&self, session: &Session) -> Result<String> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let claims = Claims {
            sub: session.id,
            email: session.email.clone(),
            role: session.role,
            exp: (now + self.ttl).as_secs() as usize,
        };
        encode(&Header::default(), &claims, &self.encoding)
            .map_err(|err| Error::internal(anyhow::anyhow!("no se pudo firmar el token: {err}")))
    }

    fn verify(&self, token: &str) -> Result<Option<Session>> {
        match decode::<Claims>(token, &self.decoding, &self.validation) {
            Ok(data) => Ok(Some(Session {
                id: data.claims.sub,
                email: data.claims.email,
                role: data.claims.role,
            })),
            // Mal formado, mal firmado, expirado o de otro algoritmo: todo es
            // «no hay identidad». `JwtMiddleware` sigue sin sesión y quien
            // rechaza es `RequireAuth`, no el codec (§26.3).
            Err(_) => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &str = "secreto-de-test-con-longitud-suficiente";

    fn tokens() -> JwtTokens {
        JwtTokens::new(SECRET, Duration::from_secs(60))
    }

    fn session() -> Session {
        Session {
            id: Uuid::new_v4(),
            email: "ana@correo.com".to_owned(),
            role: Role::Admin,
        }
    }

    #[test]
    fn ida_y_vuelta_de_la_identidad() {
        let tokens = tokens();
        let identity = session();
        let token = tokens.issue(&identity).unwrap();
        assert_eq!(tokens.verify(&token).unwrap(), Some(identity));
    }

    #[test]
    fn un_token_expirado_no_da_identidad() {
        let tokens = tokens();
        let claims = Claims {
            sub: Uuid::new_v4(),
            email: "ana@correo.com".to_owned(),
            role: Role::User,
            exp: 1,
        };
        let token = encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(SECRET.as_bytes()),
        )
        .unwrap();
        assert_eq!(tokens.verify(&token).unwrap(), None);
    }

    #[test]
    fn otro_secreto_o_un_algoritmo_distinto_no_verifican() {
        let tokens = tokens();
        let identity = session();
        let claims = Claims {
            sub: identity.id,
            email: identity.email.clone(),
            role: identity.role,
            exp: usize::MAX,
        };

        let ajeno = encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(b"otro-secreto"),
        )
        .unwrap();
        assert_eq!(tokens.verify(&ajeno).unwrap(), None);

        let header = Header::new(Algorithm::HS384);
        let otro_alg = encode(
            &header,
            &claims,
            &EncodingKey::from_secret(SECRET.as_bytes()),
        )
        .unwrap();
        assert_eq!(tokens.verify(&otro_alg).unwrap(), None);

        assert_eq!(tokens.verify("no-es-un-jwt").unwrap(), None);
        assert_eq!(tokens.verify("").unwrap(), None);
    }

    #[test]
    fn el_rol_viaja_en_el_token() {
        let tokens = tokens();
        let mut identity = session();
        identity.role = Role::User;
        let token = tokens.issue(&identity).unwrap();
        assert_eq!(tokens.verify(&token).unwrap().unwrap().role, Role::User);
    }
}
