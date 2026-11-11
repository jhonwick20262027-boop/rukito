//! Módulo `users` (≈ `users.module.ts`, §5, §26.2).
//!
//! Raíz de composición: declara las cuatro capas, fija los adaptadores concretos
//! y escribe las impls de DI que `#[service]` generará (§6.3). Al no estar dentro
//! de ninguna capa, es el único archivo del módulo que puede nombrarlas a todas
//! sin violar RA-001 a RA-003 (§17).

pub mod application;
pub mod domain;
pub mod infrastructure;
pub mod presentation;

use std::sync::Arc;

use rdd::module;
use rdd::prelude::*;

use self::application::users_use_cases::UsersUseCases;
use self::domain::User;
use self::infrastructure::argon2_hasher::Argon2Hasher;
use self::infrastructure::users_repository::UsersRepository;
use self::presentation::users_controller::UsersController;

/// API pública del módulo (RA-004): lo único que `src/middleware/` y
/// `modules/tasks/` pueden importar de aquí. Los archivos internos de cada capa
/// quedan fuera de su alcance.
pub use self::domain::{Role, Session, TokenCodec};
pub use self::infrastructure::jwt_tokens::JwtTokens;

/// El caso de uso con sus adaptadores reales. La capa HTTP importa este alias en
/// vez del adaptador, que RA-003 le prohíbe (§14.2).
pub type UsersService = UsersUseCases<UsersRepository>;

/// El codec HS256 con la configuración del entorno. Lo llama `main.rs` una vez y
/// se lo pasa al módulo; el contenedor reparte esa misma instancia.
pub fn tokens() -> JwtTokens {
    JwtTokens::new(
        &crate::config::jwt_config::secret(),
        crate::config::jwt_config::ttl(),
    )
}

/// Singleton del contenedor: sin él, cada petición volvería a leer el entorno y
/// a reconstruir las claves.
impl FromContainer for JwtTokens {
    const NAME: &'static str = "JwtTokens";

    fn from_container(container: &Container) -> Result<Self> {
        container.get::<Self>().ok_or_else(|| {
            Error::not_found("JwtTokens no está registrado: monta `users::module(tokens)` (§6.2)")
        })
    }
}

impl FromContainer for UsersService {
    const NAME: &'static str = "UsersService";

    fn from_container(container: &Container) -> Result<Self> {
        let repo = container.resolve::<Repo<User>>("UsersService", "repo")?;
        let tokens = container.resolve::<JwtTokens>("UsersService", "tokens")?;
        Ok(Self::new(
            UsersRepository::new(repo),
            Arc::new(Argon2Hasher),
            Arc::new(tokens),
            crate::config::jwt_config::bootstrap_admin(),
        ))
    }
}

impl FromRequest for UsersService {
    async fn from_request(ctx: &Ctx, _bind: Bind) -> Result<Self> {
        ctx.resolve::<Self>()
    }
}

/// El módulo listo para montar (§6.2).
///
/// El codec entra por parámetro y no se lee del entorno aquí dentro: así un test
/// puede montar la app con un secreto propio sin tocar variables de proceso
/// (RF-160). `providers:` de la macro va vacío porque `App::build()` ya registra
/// el valor `Db` y `Repo<T>` lo lee del contenedor (§12.1); el codec sí necesita
/// registro explícito porque no sale de la base de datos.
pub fn module(tokens: JwtTokens) -> rdd::Module {
    module! {
        providers: [],
        controllers: [UsersController],
        middlewares: [],
    }
    .provider(ProviderReg::new(vec![], move |_| Ok(tokens.clone())))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    const SECRET: &str = "secreto-de-test-del-modulo-users";

    /// Cada test recibe su propio backend y su propio bootstrap: no hay estado
    /// global que compartir (RF-160). Aquí sí se pueden nombrar los adaptadores,
    /// porque este archivo es la raíz de composición y no una capa (§17).
    fn service(bootstrap_admin: Option<&str>) -> UsersService {
        let repo = Repo::<User>::new(Arc::new(MemoryBackend::new()));
        UsersService::new(
            UsersRepository::new(repo),
            Arc::new(Argon2Hasher),
            Arc::new(JwtTokens::new(SECRET, Duration::from_secs(60))),
            bootstrap_admin.map(str::to_owned),
        )
    }

    #[tokio::test]
    async fn registra_y_autentica() {
        let svc = service(None);
        let user = svc
            .register("Ana@Correo.com".to_owned(), "larga123".to_owned())
            .await
            .unwrap();
        assert_eq!(user.email, "ana@correo.com");
        assert_eq!(user.role, Role::User);
        assert!(user.password_hash.starts_with("$argon2id$"));

        let (login, token) = svc
            .login("ana@correo.com".to_owned(), "larga123".to_owned())
            .await
            .unwrap();
        assert_eq!(login.id, user.id);
        assert!(!token.is_empty());
    }

    #[tokio::test]
    async fn el_token_devuelve_la_identidad_completa() {
        let svc = service(None);
        let user = svc
            .register("ana@correo.com".to_owned(), "larga123".to_owned())
            .await
            .unwrap();
        let (_, token) = svc
            .login("ana@correo.com".to_owned(), "larga123".to_owned())
            .await
            .unwrap();

        let codec = JwtTokens::new(SECRET, Duration::from_secs(60));
        assert_eq!(codec.verify(&token).unwrap(), Some(user.session()));
    }

    #[tokio::test]
    async fn el_correo_duplicado_es_409() {
        let svc = service(None);
        svc.register("ana@correo.com".to_owned(), "larga123".to_owned())
            .await
            .unwrap();
        let err = svc
            .register("ANA@correo.com".to_owned(), "otra-clave".to_owned())
            .await
            .unwrap_err();
        assert_eq!(err.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn credenciales_malas_son_401_sin_delatar_si_la_cuenta_existe() {
        let svc = service(None);
        svc.register("ana@correo.com".to_owned(), "larga123".to_owned())
            .await
            .unwrap();

        let clave_mala = svc
            .login("ana@correo.com".to_owned(), "equivocada".to_owned())
            .await
            .unwrap_err();
        let cuenta_inexistente = svc
            .login("nadie@correo.com".to_owned(), "larga123".to_owned())
            .await
            .unwrap_err();
        assert_eq!(clave_mala.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(cuenta_inexistente.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn valida_correo_y_contraseña() {
        let svc = service(None);
        let correo = svc
            .register("no-es-un-correo".to_owned(), "larga123".to_owned())
            .await
            .unwrap_err();
        assert_eq!(correo.status(), StatusCode::UNPROCESSABLE_ENTITY);

        let clave = svc
            .register("ana@correo.com".to_owned(), "corta".to_owned())
            .await
            .unwrap_err();
        assert_eq!(clave.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn solo_el_correo_configurado_nace_admin() {
        let svc = service(Some("jefa@correo.com"));
        let admin = svc
            .register("JEFA@correo.com".to_owned(), "larga123".to_owned())
            .await
            .unwrap();
        let normal = svc
            .register("ana@correo.com".to_owned(), "larga123".to_owned())
            .await
            .unwrap();
        assert_eq!(admin.role, Role::Admin);
        assert_eq!(normal.role, Role::User);
    }

    #[tokio::test]
    async fn me_lee_la_cuenta_de_la_sesion() {
        let svc = service(None);
        let user = svc
            .register("ana@correo.com".to_owned(), "larga123".to_owned())
            .await
            .unwrap();
        assert_eq!(svc.me(user.id).await.unwrap().email, user.email);
        assert_eq!(
            svc.me(uuid::Uuid::new_v4()).await.unwrap_err().status(),
            StatusCode::NOT_FOUND
        );
    }
}
