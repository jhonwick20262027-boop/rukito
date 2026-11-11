//! Controller de `users` (§8.2, RA-003, §26.2).
//!
//! Publica `/auth/register`, `/auth/login` y `/users/me`. El prefijo va vacío
//! porque las tres no comparten uno solo; `joined_path` normaliza (RF-051).
//! Solo `/users/me` exige sesión: registro y login tienen que seguir públicos
//! para que `RequireAuth` no encierre la app en un callejón (§26.4).

use rdd::controller;
use rdd::prelude::*;

use crate::middleware::RequireAuth;
use crate::modules::users::presentation::dto::{
    LoginDto, RegisterDto, TokenResponse, UserResponse,
};
use crate::modules::users::{Session, UsersService};

pub struct UsersController;

#[controller("")]
impl UsersController {
    /// `POST /auth/register` — 201 con la cuenta creada, sin el hash.
    #[post("/auth/register", status = 201)]
    async fn register(svc: UsersService, body: Json<RegisterDto>) -> Result<UserResponse> {
        let body = body.into_inner();
        svc.register(body.email, body.password)
            .await
            .map(UserResponse::from)
    }

    /// `POST /auth/login` — 200 con el JWT; 401 si las credenciales no sirven.
    // `status = 200` explícito: el 201 por omisión de POST no aplica aquí, entrar
    // no crea un recurso (§26.6, comprobación 2).
    #[post("/auth/login", status = 200)]
    async fn login(svc: UsersService, body: Json<LoginDto>) -> Result<TokenResponse> {
        let body = body.into_inner();
        let (user, token) = svc.login(body.email, body.password).await?;
        Ok(TokenResponse {
            access_token: token,
            token_type: "Bearer",
            user: user.into(),
        })
    }

    /// `GET /users/me` — la cuenta de la sesión, nunca la de un id de ruta.
    #[get("/users/me")]
    #[middleware(RequireAuth)]
    async fn me(svc: UsersService, session: Ext<Session>) -> Result<UserResponse> {
        svc.me(session.id).await.map(UserResponse::from)
    }
}
