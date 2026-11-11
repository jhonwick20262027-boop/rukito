//! El árbol modular mínimo de §5: domain, application, infrastructure y presentation.
//!
//! Lo usan por igual `rdd new` (módulo de ejemplo `tasks`) y `rdd g module`, así
//! que el módulo de ejemplo no es un caso especial: es este mismo generador.
//!
//! Las plantillas usan `@token@` en vez de `format!` para no tener que escapar
//! cada llave del código Rust generado.

use crate::files::GeneratedFile;
use crate::naming::{pascal_case, singular, snake_case};
use crate::templates::render;

/// Nombres derivados del módulo, según §5 y RF-005.
pub struct ModuleNames {
    /// Directorio y plural: `tasks`, `task_items`.
    pub dir: String,
    /// Singular del agregado: `task`, `task_item`.
    pub single: String,
    /// `Tasks`, `TaskItems`.
    pub plural_ty: String,
    /// `Task`, `TaskItem`.
    pub single_ty: String,
}

impl ModuleNames {
    pub fn new(name: &str) -> Self {
        let dir = snake_case(name);
        let single = singular(&dir);
        Self {
            plural_ty: pascal_case(&dir),
            single_ty: pascal_case(&single),
            dir,
            single,
        }
    }

    fn vars(&self) -> [(&'static str, &str); 4] {
        [
            ("plural", &self.dir),
            ("single", &self.single),
            ("Plural", &self.plural_ty),
            ("Single", &self.single_ty),
        ]
    }
}

/// Todos los archivos del módulo, con rutas relativas a `src/modules/<dir>/`.
pub fn files(names: &ModuleNames) -> Vec<GeneratedFile> {
    let vars = names.vars();
    let raw = [
        ("mod.rs", MOD_RS),
        ("domain/mod.rs", DOMAIN_MOD),
        ("domain/@single@.rs", DOMAIN),
        ("domain/@plural@_repository.rs", DOMAIN_REPOSITORY),
        ("application/mod.rs", APPLICATION_MOD),
        ("application/@plural@_use_cases.rs", USE_CASES),
        ("infrastructure/mod.rs", INFRASTRUCTURE_MOD),
        ("infrastructure/@plural@_repository.rs", REPOSITORY),
        ("presentation/mod.rs", PRESENTATION_MOD),
        ("presentation/dto.rs", DTO),
        ("presentation/@plural@_controller.rs", CONTROLLER),
    ];

    raw.iter()
        .map(|(path, body)| GeneratedFile::new(render(path, &vars), render(body, &vars)))
        .collect()
}

const MOD_RS: &str = r#"//! Módulo `@plural@` (≈ `@plural@.module.ts`, §5).
//!
//! Raíz de composición: declara las cuatro capas, fija el adaptador concreto del
//! caso de uso y escribe las dos impls de DI que `#[service]` generará (§6.3).
//! Al no estar dentro de ninguna capa, es el único archivo del módulo que puede
//! nombrarlas a todas sin violar RA-001 a RA-003 (§17).

pub mod application;
pub mod domain;
pub mod infrastructure;
pub mod presentation;

use rdd::module;
use rdd::prelude::*;

use self::application::@plural@_use_cases::@Plural@UseCases;
use self::domain::@Single@;
use self::infrastructure::@plural@_repository::@Plural@Repository;
use self::presentation::@plural@_controller::@Plural@Controller;

/// El caso de uso con su adaptador real. La capa HTTP importa este alias en vez
/// del adaptador, que RA-003 le prohíbe (§14.2).
pub type @Plural@Service = @Plural@UseCases<@Plural@Repository>;

impl FromContainer for @Plural@Service {
    const NAME: &'static str = "@Plural@Service";

    fn from_container(container: &Container) -> Result<Self> {
        let repo = container.resolve::<Repo<@Single@>>("@Plural@Service", "repo")?;
        Ok(Self::with_repository(@Plural@Repository::new(repo)))
    }
}

impl FromRequest for @Plural@Service {
    async fn from_request(ctx: &Ctx, _bind: Bind) -> Result<Self> {
        ctx.resolve::<Self>()
    }
}

/// El módulo listo para montar (§6.2).
///
/// `providers:` va vacío: `App::build()` registra el valor `Db` y `Repo<T>` lo lee
/// del contenedor (§12.1), así que no hay que declarar el repositorio como provider.
pub fn module() -> rdd::Module {
    module! {
        providers: [],
        controllers: [@Plural@Controller],
        middlewares: [],
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    /// Cada test recibe su propio backend: no hay estado global que compartir
    /// entre tests (RF-160). Aquí sí se puede nombrar el adaptador, porque este
    /// archivo es la raíz de composición y no una capa (§17).
    fn service() -> @Plural@Service {
        let repo = Repo::<@Single@>::new(Arc::new(MemoryBackend::new()));
        @Plural@Service::with_repository(@Plural@Repository::new(repo))
    }

    #[tokio::test]
    async fn creates_and_retrieves_an_item() {
        let svc = service();
        let created = svc.create("pan".to_owned(), None).await.unwrap();
        assert_eq!(svc.get(created.id).await.unwrap().title, "pan");
    }

    #[tokio::test]
    async fn rejects_empty_titles() {
        let err = service().create("  ".to_owned(), None).await.unwrap_err();
        assert_eq!(err.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn filters_items() {
        let svc = service();
        svc.create("pan".to_owned(), None).await.unwrap();
        svc.create("café".to_owned(), Some(true)).await.unwrap();

        let found = svc.list(Some("ca".to_owned()), Some(true)).await.unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].title, "café");
    }

    #[tokio::test]
    async fn updates_and_removes_an_item() {
        let svc = service();
        let created = svc.create("pan".to_owned(), None).await.unwrap();
        let updated = svc.update(created.id, None, Some(true)).await.unwrap();
        assert!(updated.done);
        assert_eq!(updated.title, "pan");

        svc.remove(created.id).await.unwrap();
        assert_eq!(
            svc.get(created.id).await.unwrap_err().status(),
            StatusCode::NOT_FOUND
        );
    }
}
"#;

const CONTROLLER: &str = r#"//! Controller de `@plural@` (§8.2, RA-003).
//!
//! Traduce HTTP a llamadas de aplicación y convierte el resultado del dominio
//! al DTO de respuesta. Usa el alias público del módulo, nunca el adaptador de
//! persistencia (§14.2).

use rdd::controller;
use rdd::prelude::*;
use uuid::Uuid;

use crate::modules::@plural@::@Plural@Service;
use crate::modules::@plural@::presentation::dto::{
    Create@Single@Dto, Query@Single@Dto, @Single@Response, Update@Single@Dto,
};

pub struct @Plural@Controller;

#[controller("/@plural@")]
impl @Plural@Controller {
    /// `GET /@plural@` — lista, con filtro opcional por texto y estado.
    #[get("/")]
    async fn list(
        svc: @Plural@Service,
        query: Query<Query@Single@Dto>,
    ) -> Result<Vec<@Single@Response>> {
        let query = query.into_inner();
        Ok(svc
            .list(query.q, query.done)
            .await?
            .into_iter()
            .map(@Single@Response::from)
            .collect())
    }

    /// `GET /@plural@/:id` — 404 en Problem Details si no existe (§10.3).
    #[get("/:id")]
    async fn get(svc: @Plural@Service, id: Path<Uuid>) -> Result<@Single@Response> {
        svc.get(*id).await.map(@Single@Response::from)
    }

    /// `POST /@plural@` — 201 con el recurso creado (RF-053).
    #[post("/", status = 201)]
    async fn create(
        svc: @Plural@Service,
        body: Json<Create@Single@Dto>,
    ) -> Result<@Single@Response> {
        let body = body.into_inner();
        svc.create(body.title, body.done)
            .await
            .map(@Single@Response::from)
    }

    /// `PUT /@plural@/:id` — actualización parcial.
    #[put("/:id")]
    async fn update(
        svc: @Plural@Service,
        id: Path<Uuid>,
        body: Json<Update@Single@Dto>,
    ) -> Result<@Single@Response> {
        let body = body.into_inner();
        svc.update(*id, body.title, body.done)
            .await
            .map(@Single@Response::from)
    }

    /// `DELETE /@plural@/:id` — 204 sin cuerpo (RF-053).
    #[delete("/:id")]
    async fn remove(svc: @Plural@Service, id: Path<Uuid>) -> Result<()> {
        svc.remove(*id).await
    }
}
"#;

const APPLICATION_MOD: &str = r#"//! Casos de uso de `@plural@` (§5.1, RA-002).
//!
//! Coordina dominio y puertos; no contiene reglas de negocio.

pub mod @plural@_use_cases;
"#;

const USE_CASES: &str = r#"//! Casos de uso de `@plural@` (§5.1, RA-002).
//!
//! Coordina operaciones de dominio y expone una API independiente de HTTP.

use rdd::prelude::*;
use uuid::Uuid;

use crate::modules::@plural@::domain::@Single@Repository;
use crate::modules::@plural@::domain::{@Single@, @Single@Error};

/// Orquesta los casos de uso; las invariantes pertenecen al agregado.
///
/// Sin tipo por defecto: nombrar el adaptador aquí violaría RA-002. Quien lo fija
/// es la raíz de composición del módulo con el alias `@Plural@Service` (§14.2).
pub struct @Plural@UseCases<R: @Single@Repository> {
    repo: R,
}

impl<R: @Single@Repository> @Plural@UseCases<R> {
    /// Constructor sobre cualquier implementación del puerto del dominio.
    pub fn with_repository(repo: R) -> Self {
        Self { repo }
    }

    pub async fn list(&self, q: Option<String>, done: Option<bool>) -> Result<Vec<@Single@>> {
        Ok(self
            .repo
            .list()
            .await?
            .into_iter()
            .filter(|row| match &q {
                Some(text) => row.title.contains(text.as_str()),
                None => true,
            })
            .filter(|row| done.is_none_or(|done| row.done == done))
            .collect())
    }

    pub async fn get(&self, id: Uuid) -> Result<@Single@> {
        self.repo.get(id).await
    }

    pub async fn create(&self, title: String, done: Option<bool>) -> Result<@Single@> {
        let item = @Single@::new(title, done.unwrap_or(false)).map_err(domain_error)?;
        self.repo.save(&item).await?;
        Ok(item)
    }

    pub async fn update(
        &self,
        id: Uuid,
        title: Option<String>,
        done: Option<bool>,
    ) -> Result<@Single@> {
        let mut current = self.repo.get(id).await?;
        current.update(title, done).map_err(domain_error)?;
        self.repo.save(&current).await?;
        Ok(current)
    }

    pub async fn remove(&self, id: Uuid) -> Result<()> {
        self.repo.remove(id).await
    }
}

fn domain_error(error: @Single@Error) -> Error {
    match error {
        @Single@Error::EmptyTitle => Error::bad_request("el título no puede estar vacío"),
        @Single@Error::TitleTooLong { max } => {
            Error::invalid(format!("el título no puede pasar de {max} caracteres"))
        }
    }
}
"#;

const INFRASTRUCTURE_MOD: &str = r#"//! Adaptadores técnicos del módulo `@plural@` (§5.1, RA-007).
//!
//! Implementan los puertos definidos por el dominio.

pub mod @plural@_repository;
"#;

const REPOSITORY: &str = r#"//! Adaptador de persistencia de `@Single@` sobre `Repo<T>` (§12.1, RA-007).
//!
//! No conoce ningún driver: el motor lo decide la URL en el arranque (§12.6), así
//! que cambiar de PostgreSQL a otro no toca este archivo ni el caso de uso
//! (RF-185). Tampoco tiene almacén propio ni `static`: el `Repo` se inyecta.

use rdd::prelude::*;
use uuid::Uuid;

use crate::modules::@plural@::domain::@Single@Repository;
use crate::modules::@plural@::domain::@Single@;

/// Implementa el puerto del dominio con el handle agnóstico del núcleo.
pub struct @Plural@Repository {
    repo: Repo<@Single@>,
}

impl @Plural@Repository {
    /// Recibe el `Repo` ya resuelto del contenedor (§12.1).
    pub fn new(repo: Repo<@Single@>) -> Self {
        Self { repo }
    }
}

impl @Single@Repository for @Plural@Repository {
    async fn list(&self) -> Result<Vec<@Single@>> {
        self.repo.all().await
    }

    async fn get(&self, id: Uuid) -> Result<@Single@> {
        self.repo.get(id).await
    }

    async fn save(&self, item: &@Single@) -> Result<()> {
        self.repo.save(item).await
    }

    async fn remove(&self, id: Uuid) -> Result<()> {
        self.repo.delete(id).await
    }
}
"#;

const DOMAIN_MOD: &str = r#"//! Modelo y puertos del dominio `@plural@` (§5.1, RA-001).

pub mod @single@;
pub mod @plural@_repository;

pub use @single@::{@Single@, @Single@Error};
pub use @plural@_repository::@Single@Repository;
"#;

const DOMAIN_REPOSITORY: &str = r#"//! Puerto de persistencia del agregado `@Single@` (§5.1, RA-001).
//!
//! Async con `impl Future + Send`, la misma forma de `FromRequest` (§6.3): un
//! `async fn` en el trait no garantiza que el futuro sea `Send`, y los handlers de
//! la plataforma sí lo exigen. El adaptador lo implementa con `async fn`.

use std::future::Future;

use rdd::Result;
use uuid::Uuid;

use crate::modules::@plural@::domain::@Single@;

pub trait @Single@Repository: Send + Sync + 'static {
    fn list(&self) -> impl Future<Output = Result<Vec<@Single@>>> + Send;
    fn get(&self, id: Uuid) -> impl Future<Output = Result<@Single@>> + Send;
    fn save(&self, item: &@Single@) -> impl Future<Output = Result<()>> + Send;
    fn remove(&self, id: Uuid) -> impl Future<Output = Result<()>> + Send;
}
"#;

const DOMAIN: &str = r#"//! Agregado `@Single@` e invariantes del dominio (§7.1, §12.2).
//!
//! `Aggregate` y `Persist` se escriben a mano hasta que `#[aggregate]` exista
//! (RF-006); la macro las sustituirá sin tocar el resto del módulo. El agregado no
//! deriva `Deserialize` (RF-031): la entrada HTTP son los DTOs, no él.

use rdd::db::{Persist, Row, Value};
use rdd::domain::Aggregate;
use uuid::Uuid;

pub const MAX_TITLE_LEN: usize = 200;

#[derive(Clone, Debug, PartialEq)]
pub struct @Single@ {
    pub id: Uuid,
    pub title: String,
    pub done: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum @Single@Error {
    EmptyTitle,
    TitleTooLong { max: usize },
}

impl @Single@ {
    /// Crea el agregado con un id nuevo, validando las invariantes.
    pub fn new(title: String, done: bool) -> Result<Self, @Single@Error> {
        Ok(Self {
            id: Uuid::new_v4(),
            title: validate_title(title)?,
            done,
        })
    }

    pub fn update(
        &mut self,
        title: Option<String>,
        done: Option<bool>,
    ) -> Result<(), @Single@Error> {
        if let Some(title) = title {
            self.title = validate_title(title)?;
        }
        if let Some(done) = done {
            self.done = done;
        }
        Ok(())
    }
}

impl Aggregate for @Single@ {
    type Id = Uuid;

    fn id(&self) -> &Self::Id {
        &self.id
    }
}

impl Persist for @Single@ {
    const TABLE: &'static str = "@plural@";
    const COLUMNS: &'static [&'static str] = &["id", "title", "done"];

    fn to_values(&self) -> Vec<Value> {
        vec![
            Value::Uuid(self.id),
            Value::from(self.title.clone()),
            Value::Bool(self.done),
        ]
    }

    fn from_row(row: &Row) -> rdd::Result<Self> {
        // Por nombre y no por posición: el orden de `Row` no es contractual (§12.1).
        Ok(Self {
            id: row.value("id")?.as_uuid()?,
            title: row.value("title")?.as_text()?.to_owned(),
            done: row.value("done")?.as_bool()?,
        })
    }
}

fn validate_title(title: String) -> Result<String, @Single@Error> {
    let title = title.trim();
    if title.is_empty() {
        return Err(@Single@Error::EmptyTitle);
    }
    if title.chars().count() > MAX_TITLE_LEN {
        return Err(@Single@Error::TitleTooLong {
            max: MAX_TITLE_LEN,
        });
    }
    Ok(title.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trims_the_title_and_rejects_an_empty_one() {
        let item = @Single@::new("  pan  ".to_owned(), false).unwrap();
        assert_eq!(item.title, "pan");
        assert_eq!(
            @Single@::new("   ".to_owned(), false).unwrap_err(),
            @Single@Error::EmptyTitle
        );
    }

    #[test]
    fn rejects_a_title_over_the_limit() {
        let long = "x".repeat(MAX_TITLE_LEN + 1);
        assert_eq!(
            @Single@::new(long, false).unwrap_err(),
            @Single@Error::TitleTooLong {
                max: MAX_TITLE_LEN
            }
        );
    }

    #[test]
    fn survives_a_round_trip_through_a_row() {
        let item = @Single@::new("pan".to_owned(), true).unwrap();
        let row = Row::zip(<@Single@ as Persist>::COLUMNS, &item.to_values()).unwrap();
        assert_eq!(<@Single@ as Persist>::from_row(&row).unwrap(), item);
    }
}
"#;

const PRESENTATION_MOD: &str = r#"//! Adaptadores de entrada HTTP y DTOs del módulo `@plural@` (§5.1, RA-003).

pub mod dto;
pub mod @plural@_controller;
"#;

const DTO: &str = r#"//! Tipos de entrada y salida HTTP para `@plural@`.
//!
//! El agregado no se deserializa desde la petición (RF-031): estos DTOs son la
//! única forma de entrada, y `@Single@Response` la de salida.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::modules::@plural@::domain::@Single@;

#[derive(Debug, Deserialize)]
pub struct Query@Single@Dto {
    pub q: Option<String>,
    pub done: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct Create@Single@Dto {
    pub title: String,
    pub done: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct Update@Single@Dto {
    pub title: Option<String>,
    pub done: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct @Single@Response {
    pub id: Uuid,
    pub title: String,
    pub done: bool,
}

impl From<@Single@> for @Single@Response {
    fn from(item: @Single@) -> Self {
        Self {
            id: item.id,
            title: item.title,
            done: item.done,
        }
    }
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_only_the_four_module_layers() {
        let names = ModuleNames::new("Tasks");
        let paths: Vec<String> = files(&names)
            .iter()
            .map(|file| file.path.display().to_string().replace('\\', "/"))
            .collect();
        assert_eq!(paths.len(), 11);
        for folder in ["domain", "application", "infrastructure", "presentation"] {
            assert!(
                paths.contains(&format!("{folder}/mod.rs")),
                "falta {folder}/mod.rs"
            );
        }
        for removed in [
            "controllers",
            "repositories",
            "dto",
            "entities",
            "interfaces",
            "mappers",
            "constants",
            "tests",
        ] {
            assert!(
                !paths
                    .iter()
                    .any(|path| path.starts_with(&format!("{removed}/")))
            );
        }
        assert!(paths.contains(&"domain/task.rs".to_owned()));
        assert!(paths.contains(&"application/tasks_use_cases.rs".to_owned()));
        assert!(paths.contains(&"infrastructure/tasks_repository.rs".to_owned()));
        assert!(paths.contains(&"presentation/dto.rs".to_owned()));
        assert!(paths.contains(&"presentation/tasks_controller.rs".to_owned()));
    }

    #[test]
    fn singularizes_multiword_modules() {
        let names = ModuleNames::new("task-items");
        assert_eq!(names.dir, "task_items");
        assert_eq!(names.single, "task_item");
        assert_eq!(names.plural_ty, "TaskItems");
        assert_eq!(names.single_ty, "TaskItem");
        let paths: Vec<String> = files(&names)
            .iter()
            .map(|file| file.path.display().to_string().replace('\\', "/"))
            .collect();
        assert!(paths.contains(&"presentation/dto.rs".to_owned()));
        assert!(paths.contains(&"presentation/task_items_controller.rs".to_owned()));
        assert!(paths.contains(&"application/task_items_use_cases.rs".to_owned()));
    }

    #[test]
    fn leaves_no_placeholder_unreplaced() {
        let names = ModuleNames::new("tasks");
        for file in files(&names) {
            assert!(
                !file.contents.contains('@'),
                "quedó un token sin sustituir en {}",
                file.path.display()
            );
        }
    }

    /// RA-001 a RA-003 sobre las rutas que de verdad importan, no sobre la prosa
    /// de las cabeceras. El cableado legal vive en `mod.rs` (§17).
    #[test]
    fn keeps_the_layers_inside_their_import_rules() {
        let names = ModuleNames::new("tasks");
        let generated = files(&names);
        let find = |path: &str| {
            generated
                .iter()
                .find(|file| file.path.display().to_string().replace('\\', "/") == path)
                .unwrap_or_else(|| panic!("falta {path}"))
                .contents
                .clone()
        };
        let assert_absent = |body: &str, needles: &[&str]| {
            for needle in needles {
                assert!(!body.contains(needle), "importa `{needle}`:\n{body}");
            }
        };

        let domain = find("domain/task.rs");
        let port = find("domain/tasks_repository.rs");
        for body in [&domain, &port] {
            assert_absent(
                body,
                &[
                    "crate::modules::tasks::application",
                    "crate::modules::tasks::infrastructure",
                    "crate::modules::tasks::presentation",
                    "rdd::http",
                    "axum",
                    "sqlx",
                ],
            );
        }

        assert_absent(
            &find("application/tasks_use_cases.rs"),
            &[
                "crate::modules::tasks::infrastructure",
                "crate::modules::tasks::presentation",
                "rdd::http",
                "axum",
                "sqlx",
            ],
        );
        assert_absent(
            &find("presentation/tasks_controller.rs"),
            &[
                "crate::modules::tasks::infrastructure",
                "Repo<",
                "axum",
                "sqlx",
            ],
        );

        let root = find("mod.rs");
        assert!(
            root.contains("impl FromContainer for TasksService"),
            "{root}"
        );
        assert!(root.contains("impl FromRequest for TasksService"), "{root}");
        assert!(root.contains("MemoryBackend"), "{root}");
    }
}
