//! El árbol de módulo de §5: nueve carpetas, cada una con su `mod.rs` (RF-001).
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
    /// Singular para entities/dto/interfaces/mappers: `task`, `task_item`.
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
        ("controllers/mod.rs", CONTROLLERS_MOD),
        ("controllers/@plural@_controller.rs", CONTROLLER),
        ("services/mod.rs", SERVICES_MOD),
        ("services/@plural@_service.rs", SERVICE),
        ("repositories/mod.rs", REPOSITORIES_MOD),
        ("repositories/@plural@_repository.rs", REPOSITORY),
        ("dto/mod.rs", DTO_MOD),
        ("dto/create_@single@_dto.rs", CREATE_DTO),
        ("dto/update_@single@_dto.rs", UPDATE_DTO),
        ("dto/query_@single@_dto.rs", QUERY_DTO),
        ("entities/mod.rs", ENTITIES_MOD),
        ("entities/@single@_entity.rs", ENTITY),
        ("interfaces/mod.rs", INTERFACES_MOD),
        ("interfaces/@single@_interface.rs", INTERFACE),
        ("mappers/mod.rs", MAPPERS_MOD),
        ("mappers/@single@_mapper.rs", MAPPER),
        ("constants/mod.rs", CONSTANTS_MOD),
        ("constants/@plural@_constants.rs", CONSTANTS),
        ("tests/mod.rs", TESTS_MOD),
        ("tests/@plural@_service_test.rs", SERVICE_TEST),
        ("tests/@plural@_controller_test.rs", CONTROLLER_TEST),
    ];

    raw.iter()
        .map(|(path, body)| GeneratedFile::new(render(path, &vars), render(body, &vars)))
        .collect()
}

const MOD_RS: &str = r#"//! Módulo `@plural@` (≈ `@plural@.module.ts`, §5).
//!
//! Punto de montaje del dominio: `App::module(modules::@plural@::module())`.
//! En 0.2 los services entran por `providers` con `#[service]` y DI (§9);
//! mientras tanto `providers` va vacío y el controller construye el service
//! (RF-006).

pub mod constants;
pub mod controllers;
pub mod dto;
pub mod entities;
pub mod interfaces;
pub mod mappers;
pub mod repositories;
pub mod services;

#[cfg(test)]
mod tests;

use rdd::module;

use self::controllers::@plural@_controller::@Plural@Controller;

/// El módulo listo para montar (§6.2).
pub fn module() -> rdd::Module {
    module! {
        providers: [],
        controllers: [@Plural@Controller],
        middlewares: [],
    }
}
"#;

const CONTROLLERS_MOD: &str = r#"//! Adaptadores HTTP de `@plural@` (§5.1, RA-003).
//!
//! Un controller extrae parámetros, llama al service y devuelve; no decide
//! negocio. Lo cubre `#[controller]` con los verbos `#[get]`, `#[post]`,
//! `#[put]`, `#[patch]` y `#[delete]` (§8).

pub mod @plural@_controller;
"#;

const CONTROLLER: &str = r#"//! Controller de `@plural@` (§8.2).
//!
//! En 0.2 el service llega por DI (`req.resolve::<T>()`, RF-084); en 0.1 se
//! construye por petición porque el estado vive en el repositorio (RF-006).

use rdd::controller;
use rdd::prelude::*;

use crate::modules::@plural@::dto::create_@single@_dto::Create@Single@Dto;
use crate::modules::@plural@::dto::query_@single@_dto::Query@Single@Dto;
use crate::modules::@plural@::dto::update_@single@_dto::Update@Single@Dto;
use crate::modules::@plural@::entities::@single@_entity::@Single@;
use crate::modules::@plural@::services::@plural@_service::@Plural@Service;

pub struct @Plural@Controller;

#[controller("/@plural@")]
impl @Plural@Controller {
    /// `GET /@plural@` — lista, con filtro opcional por texto y estado.
    #[get("/")]
    async fn list(query: Query<Query@Single@Dto>) -> Result<Vec<@Single@>> {
        @Plural@Service::new().list(query.into_inner())
    }

    /// `GET /@plural@/:id` — 404 en Problem Details si no existe (§10.3).
    #[get("/:id")]
    async fn get(id: Path<u32>) -> Result<@Single@> {
        @Plural@Service::new().get(*id)
    }

    /// `POST /@plural@` — 201 con el recurso creado (RF-053).
    #[post("/", status = 201)]
    async fn create(body: Json<Create@Single@Dto>) -> Result<@Single@> {
        @Plural@Service::new().create(body.into_inner())
    }

    /// `PUT /@plural@/:id` — actualización parcial.
    #[put("/:id")]
    async fn update(id: Path<u32>, body: Json<Update@Single@Dto>) -> Result<@Single@> {
        @Plural@Service::new().update(*id, body.into_inner())
    }

    /// `DELETE /@plural@/:id` — 204 sin cuerpo (RF-053).
    #[delete("/:id")]
    async fn remove(id: Path<u32>) -> Result<()> {
        @Plural@Service::new().remove(*id)
    }
}
"#;

const SERVICES_MOD: &str = r#"//! Casos de uso y reglas de negocio de `@plural@` (§5.1, RA-002).
//!
//! En 0.2 `#[service]` registra el tipo en el contenedor y permite inyectarlo
//! (§9); en 0.1 es Rust plano y se construye a mano (RF-006).

pub mod @plural@_service;
"#;

const SERVICE: &str = r#"//! Reglas de negocio de `@plural@`.
//!
//! El controller no decide nada: aquí se valida y se traduce a `Error`, que el
//! núcleo convierte en Problem Details (§10.3).

use rdd::prelude::*;

use crate::modules::@plural@::constants::@plural@_constants::MAX_TITLE_LEN;
use crate::modules::@plural@::dto::create_@single@_dto::Create@Single@Dto;
use crate::modules::@plural@::dto::query_@single@_dto::Query@Single@Dto;
use crate::modules::@plural@::dto::update_@single@_dto::Update@Single@Dto;
use crate::modules::@plural@::entities::@single@_entity::@Single@;
use crate::modules::@plural@::interfaces::@single@_interface::@Single@Store;
use crate::modules::@plural@::mappers::@single@_mapper::@Single@Mapper;
use crate::modules::@plural@::repositories::@plural@_repository::@Plural@Repository;

/// El service depende del puerto `@Single@Store`, no de la implementación
/// (§5.1, RA-001). El parámetro por defecto es el repositorio de la app; los
/// tests pasan uno aislado. En 0.2 esto lo inyecta el contenedor (§9).
pub struct @Plural@Service<R: @Single@Store = @Plural@Repository> {
    repo: R,
}

impl @Plural@Service {
    /// Service sobre el almacén compartido de la app.
    pub fn new() -> Self {
        Self::with_repository(@Plural@Repository::shared())
    }
}

impl<R: @Single@Store> @Plural@Service<R> {
    /// Almacén propio: es lo que usan los tests, sin estado global.
    pub fn with_repository(repo: R) -> Self {
        Self { repo }
    }

    pub fn list(&self, query: Query@Single@Dto) -> Result<Vec<@Single@>> {
        Ok(self
            .repo
            .all()
            .into_iter()
            .filter(|row| match &query.q {
                Some(text) => row.title.contains(text.as_str()),
                None => true,
            })
            .filter(|row| query.done.is_none_or(|done| row.done == done))
            .collect())
    }

    pub fn get(&self, id: u32) -> Result<@Single@> {
        self.repo
            .find(id)
            .ok_or_else(|| Error::not_found(format!("@Single@ {id} no existe")))
    }

    pub fn create(&self, dto: Create@Single@Dto) -> Result<@Single@> {
        let title = validate_title(&dto.title)?;
        let id = self.repo.next_id();
        let row = @Single@Mapper::from_create(id, title, dto.done.unwrap_or(false));
        Ok(self.repo.insert(row))
    }

    pub fn update(&self, id: u32, dto: Update@Single@Dto) -> Result<@Single@> {
        let current = self.get(id)?;
        let title = match dto.title {
            Some(title) => validate_title(&title)?,
            None => current.title.clone(),
        };
        let row = @Single@Mapper::apply_update(current, title, dto.done);
        self.repo
            .update(row)
            .ok_or_else(|| Error::not_found(format!("@Single@ {id} no existe")))
    }

    pub fn remove(&self, id: u32) -> Result<()> {
        if self.repo.remove(id) {
            Ok(())
        } else {
            Err(Error::not_found(format!("@Single@ {id} no existe")))
        }
    }
}

/// El título no va vacío ni pasa de `MAX_TITLE_LEN`.
///
/// Vacío es 400 (petición mal formada); demasiado largo es 422 (`Invalid`), que
/// es la distinción que hace §10.3.
fn validate_title(title: &str) -> Result<String> {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        return Err(Error::bad_request("el título no puede estar vacío"));
    }
    if trimmed.chars().count() > MAX_TITLE_LEN {
        return Err(Error::invalid(format!(
            "el título no puede pasar de {MAX_TITLE_LEN} caracteres"
        )));
    }
    Ok(trimmed.to_owned())
}
"#;

const REPOSITORIES_MOD: &str = r#"//! Acceso a datos de `@plural@` (§5.1).
//!
//! Implementa el puerto de `interfaces/` y habla con `infrastructure/`. En 0.2
//! envuelve `Repo<T>` (§12.1) sobre memoria o PostgreSQL; en 0.1 el almacén es
//! un `Vec` en memoria porque todavía no hay persistencia (RF-006).

pub mod @plural@_repository;
"#;

const REPOSITORY: &str = r#"//! Implementación en memoria de `@Single@Store`.
//!
//! `shared()` es el almacén de la app y `new()` uno vacío y aislado para tests.
//! En 0.2 esto pasa a `Repo<T>` sobre `MemoryBackend` o `SqlxBackend` (§12.1)
//! sin tocar el service.

use std::sync::{Arc, LazyLock, Mutex, MutexGuard};

use crate::modules::@plural@::entities::@single@_entity::@Single@;
use crate::modules::@plural@::interfaces::@single@_interface::@Single@Store;

/// Las filas del módulo, en memoria y bajo un mutex.
type Rows = Arc<Mutex<Vec<@Single@>>>;

static SHARED: LazyLock<Rows> = LazyLock::new(|| Arc::new(Mutex::new(Vec::new())));

#[derive(Clone, Default)]
pub struct @Plural@Repository {
    rows: Rows,
}

impl @Plural@Repository {
    /// Almacén vacío y aislado.
    pub fn new() -> Self {
        Self::default()
    }

    /// Almacén compartido por toda la app.
    pub fn shared() -> Self {
        Self {
            rows: Arc::clone(&SHARED),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Vec<@Single@>> {
        // Un handler que entra en pánico no debe envenenar el almacén: el 500 ya
        // lo devuelve el núcleo (RF-073).
        self.rows.lock().unwrap_or_else(|err| err.into_inner())
    }
}

impl @Single@Store for @Plural@Repository {
    fn all(&self) -> Vec<@Single@> {
        self.lock().clone()
    }

    fn find(&self, id: u32) -> Option<@Single@> {
        self.lock().iter().find(|row| row.id == id).cloned()
    }

    fn next_id(&self) -> u32 {
        self.lock().iter().map(|row| row.id).max().unwrap_or(0) + 1
    }

    fn insert(&self, row: @Single@) -> @Single@ {
        self.lock().push(row.clone());
        row
    }

    fn update(&self, row: @Single@) -> Option<@Single@> {
        let mut rows = self.lock();
        let slot = rows.iter_mut().find(|item| item.id == row.id)?;
        *slot = row.clone();
        Some(row)
    }

    fn remove(&self, id: u32) -> bool {
        let mut rows = self.lock();
        let before = rows.len();
        rows.retain(|row| row.id != id);
        rows.len() != before
    }
}
"#;

const DTO_MOD: &str = r#"//! Contratos de entrada de `@plural@` (§5.1).
//!
//! Un DTO por caso de uso. La validación de forma (tipos, campos requeridos) la
//! hacen `Json<T>` y `Query<T>` (§10); la de negocio vive en `services/`.

pub mod create_@single@_dto;
pub mod query_@single@_dto;
pub mod update_@single@_dto;
"#;

const CREATE_DTO: &str = r#"//! Cuerpo de `POST /@plural@`.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Create@Single@Dto {
    pub title: String,
    pub done: Option<bool>,
}
"#;

const UPDATE_DTO: &str = r#"//! Cuerpo de `PUT /@plural@/:id`: todo opcional, actualización parcial.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Update@Single@Dto {
    pub title: Option<String>,
    pub done: Option<bool>,
}
"#;

const QUERY_DTO: &str = r#"//! Query string de `GET /@plural@` (`Query<T>`, RF-054).

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Query@Single@Dto {
    /// Substring del título.
    pub q: Option<String>,
    /// Filtra por estado.
    pub done: Option<bool>,
}
"#;

const ENTITIES_MOD: &str = r#"//! Agregados y value objects de `@plural@` (§5.1, RA-001).
//!
//! En 0.2 `#[aggregate]` añade `Persist` (§12.2) y las reglas de dominio; en 0.1
//! es una estructura plana con `serde` (RF-006).

pub mod @single@_entity;
"#;

const ENTITY: &str = r#"//! Agregado `@Single@`.
//!
//! Los campos JSON conservan el nombre Rust (RF-071). El `id` lo asigna el
//! repositorio; en 0.4 Prisma pasa a `Uuid` (§15.2).

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct @Single@ {
    pub id: u32,
    pub title: String,
    pub done: bool,
}
"#;

const INTERFACES_MOD: &str = r#"//! Traits del dominio `@plural@` (§5.1, RA-001).
//!
//! El service depende del puerto, nunca de la implementación; así cambiar de
//! backend no toca las reglas de negocio.

pub mod @single@_interface;
"#;

const INTERFACE: &str = r#"//! Puerto de persistencia de `@plural@`.
//!
//! En 0.2 lo cubre `Repo<T>` (§12.1) y pasa a ser async; esta forma síncrona es
//! provisional a propósito, porque en 0.1 todavía no hay backend (RF-006).

use crate::modules::@plural@::entities::@single@_entity::@Single@;

pub trait @Single@Store {
    fn all(&self) -> Vec<@Single@>;
    fn find(&self, id: u32) -> Option<@Single@>;
    fn next_id(&self) -> u32;
    fn insert(&self, row: @Single@) -> @Single@;
    fn update(&self, row: @Single@) -> Option<@Single@>;
    fn remove(&self, id: u32) -> bool;
}
"#;

const MAPPERS_MOD: &str = r#"//! Transformación entidad ↔ DTO ↔ fila de `@plural@` (§5.1).
//!
//! Único sitio donde se decide qué campos viajan en cada dirección. En 0.4
//! entra aquí la fila que genera Prisma (`src/generated/`).

pub mod @single@_mapper;
"#;

const MAPPER: &str = r#"//! Mapper de `@Single@`.

use crate::modules::@plural@::entities::@single@_entity::@Single@;

pub struct @Single@Mapper;

impl @Single@Mapper {
    /// DTO de creación → agregado. El título ya viene validado del service.
    pub fn from_create(id: u32, title: String, done: bool) -> @Single@ {
        @Single@ { id, title, done }
    }

    /// Aplica una actualización parcial sobre el agregado actual.
    pub fn apply_update(current: @Single@, title: String, done: Option<bool>) -> @Single@ {
        @Single@ {
            title,
            done: done.unwrap_or(current.done),
            ..current
        }
    }
}
"#;

const CONSTANTS_MOD: &str = r#"//! Constantes del módulo `@plural@` (§5.1).
//!
//! Nombres en plural, como la carpeta: aquí van los valores del dominio, no los
//! de la app entera (esos están en `src/config/`).

pub mod @plural@_constants;
"#;

const CONSTANTS: &str = r#"//! Constantes de `@plural@`.

/// Máximo de caracteres del título; lo aplica el service.
pub const MAX_TITLE_LEN: usize = 200;
"#;

const TESTS_MOD: &str = r#"//! Tests unitarios de `@plural@` (§5).
//!
//! Una capa por archivo: el service con un almacén aislado y el controller sobre
//! su tabla de rutas. Los tests de la app entera viven en `tests/acceptance.rs`,
//! en la raíz del proyecto.

mod @plural@_controller_test;
mod @plural@_service_test;
"#;

const SERVICE_TEST: &str = r#"//! Reglas de negocio de `@Plural@Service`, con un almacén aislado por test.

use rdd::prelude::*;

use crate::modules::@plural@::constants::@plural@_constants::MAX_TITLE_LEN;
use crate::modules::@plural@::dto::create_@single@_dto::Create@Single@Dto;
use crate::modules::@plural@::dto::query_@single@_dto::Query@Single@Dto;
use crate::modules::@plural@::dto::update_@single@_dto::Update@Single@Dto;
use crate::modules::@plural@::repositories::@plural@_repository::@Plural@Repository;
use crate::modules::@plural@::services::@plural@_service::@Plural@Service;

fn service() -> @Plural@Service {
    @Plural@Service::with_repository(@Plural@Repository::new())
}

fn dto(title: &str) -> Create@Single@Dto {
    Create@Single@Dto {
        title: title.to_owned(),
        done: None,
    }
}

#[test]
fn crea_y_recupera() {
    let service = service();
    let created = service.create(dto("pan")).unwrap();
    assert_eq!(created.id, 1);
    assert_eq!(service.get(created.id).unwrap().title, "pan");
}

#[test]
fn titulo_vacio_es_400() {
    let err = service().create(dto("   ")).unwrap_err();
    assert_eq!(err.status(), StatusCode::BAD_REQUEST);
}

#[test]
fn titulo_largo_es_422() {
    let err = service()
        .create(dto(&"x".repeat(MAX_TITLE_LEN + 1)))
        .unwrap_err();
    assert_eq!(err.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[test]
fn id_inexistente_es_404() {
    let err = service().get(99).unwrap_err();
    assert_eq!(err.status(), StatusCode::NOT_FOUND);
}

#[test]
fn lista_filtra_por_texto() {
    let service = service();
    service.create(dto("pan")).unwrap();
    service.create(dto("café")).unwrap();
    let query = Query@Single@Dto {
        q: Some("ca".to_owned()),
        done: None,
    };
    let found = service.list(query).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].title, "café");
}

#[test]
fn actualiza_y_borra() {
    let service = service();
    let created = service.create(dto("pan")).unwrap();
    let changes = Update@Single@Dto {
        title: None,
        done: Some(true),
    };
    let updated = service.update(created.id, changes).unwrap();
    assert!(updated.done);
    assert_eq!(updated.title, "pan");
    service.remove(created.id).unwrap();
    let err = service.get(created.id).unwrap_err();
    assert_eq!(err.status(), StatusCode::NOT_FOUND);
}
"#;

const CONTROLLER_TEST: &str = r#"//! Tabla de rutas que genera `#[controller]` (RF-051, RF-056).

use crate::modules::@plural@::controllers::@plural@_controller::@Plural@Controller;

#[test]
fn expone_las_rutas_del_modulo() {
    let table: Vec<String> = @Plural@Controller::routes()
        .iter()
        .map(|route| format!("{} {}", route.method, route.path))
        .collect();
    assert_eq!(
        table,
        [
            "GET /@plural@",
            "GET /@plural@/:id",
            "POST /@plural@",
            "PUT /@plural@/:id",
            "DELETE /@plural@/:id",
        ]
    );
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_the_nine_folders() {
        let names = ModuleNames::new("Tasks");
        let paths: Vec<String> = files(&names)
            .iter()
            .map(|file| file.path.display().to_string().replace('\\', "/"))
            .collect();
        assert_eq!(paths.len(), 22);
        for folder in [
            "controllers",
            "services",
            "repositories",
            "dto",
            "entities",
            "interfaces",
            "mappers",
            "constants",
            "tests",
        ] {
            assert!(
                paths.contains(&format!("{folder}/mod.rs")),
                "falta {folder}/mod.rs (RF-003)"
            );
        }
        assert!(paths.contains(&"dto/create_task_dto.rs".to_owned()));
        assert!(paths.contains(&"entities/task_entity.rs".to_owned()));
        assert!(paths.contains(&"controllers/tasks_controller.rs".to_owned()));
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
        assert!(paths.contains(&"dto/create_task_item_dto.rs".to_owned()));
        assert!(paths.contains(&"services/task_items_service.rs".to_owned()));
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
}
