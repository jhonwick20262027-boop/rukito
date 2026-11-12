//! Módulo `tasks` (≈ `tasks.module.ts`, §5).
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

use self::application::tasks_use_cases::TasksUseCases;
use self::domain::Task;
use self::infrastructure::tasks_repository::TasksRepository;
use self::presentation::tasks_controller::TasksController;
use crate::middleware::TaskEvents;

/// Canal de actualización del módulo; `main` lo registra en el contenedor y en
/// `App::socket` (§10.4, §26.7).
pub use self::presentation::dto::TaskResponse;
pub use self::presentation::task_feed::TaskFeed;
/// El tipo de salida GraphQL se publica con otro nombre para no chocar con el
/// agregado `Task` (§26.9).
pub use self::presentation::task_graphql::Task as GraphqlTask;

/// El caso de uso con su adaptador real. La capa HTTP importa este alias en vez
/// del adaptador, que RA-003 le prohíbe (§14.2).
pub type TasksService = TasksUseCases<TasksRepository>;

impl FromContainer for TasksService {
    const NAME: &'static str = "TasksService";

    fn from_container(container: &Container) -> Result<Self> {
        let repo = container.resolve::<Repo<Task>>("TasksService", "repo")?;
        Ok(Self::with_repository(TasksRepository::new(repo)))
    }
}

impl FromRequest for TasksService {
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
        controllers: [TasksController],
        middlewares: [TaskEvents],
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    /// Cada test recibe su propio backend: no hay estado global que compartir
    /// entre tests (RF-160). Aquí sí se puede nombrar el adaptador, porque este
    /// archivo es la raíz de composición y no una capa (§17).
    fn service() -> TasksService {
        let repo = Repo::<Task>::new(Arc::new(MemoryBackend::new()));
        TasksService::with_repository(TasksRepository::new(repo))
    }

    fn owner() -> uuid::Uuid {
        uuid::Uuid::new_v4()
    }

    #[tokio::test]
    async fn crea_y_recupera_una_tarea() {
        let svc = service();
        let me = owner();
        let created = svc.create(me, "pan".to_owned(), None).await.unwrap();
        assert_eq!(created.owner_id, me);
        assert_eq!(svc.get_of(me, created.id).await.unwrap().title, "pan");
    }

    #[tokio::test]
    async fn rechaza_un_titulo_vacio() {
        let err = service()
            .create(owner(), "  ".to_owned(), None)
            .await
            .unwrap_err();
        assert_eq!(err.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn filtra_por_texto_y_estado() {
        let svc = service();
        let me = owner();
        svc.create(me, "pan".to_owned(), None).await.unwrap();
        svc.create(me, "café".to_owned(), Some(true)).await.unwrap();

        let found = svc
            .list_of(me, Some("ca".to_owned()), Some(true))
            .await
            .unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].title, "café");
    }

    #[tokio::test]
    async fn actualiza_y_borra() {
        let svc = service();
        let me = owner();
        let created = svc.create(me, "pan".to_owned(), None).await.unwrap();
        let updated = svc
            .update_of(me, created.id, None, Some(true))
            .await
            .unwrap();
        assert!(updated.done);
        assert_eq!(updated.title, "pan");

        svc.remove_of(me, created.id).await.unwrap();
        assert_eq!(
            svc.get_of(me, created.id).await.unwrap_err().status(),
            StatusCode::NOT_FOUND
        );
    }

    /// §26.6, comprobación 8: la tarea de otro no existe para mí, y eso es 404.
    /// Un 403 revelaría que el recurso está ahí.
    #[tokio::test]
    async fn las_tareas_de_otro_son_404_en_las_cuatro_operaciones() {
        let svc = service();
        let ajena = svc.create(owner(), "ajena".to_owned(), None).await.unwrap();
        let yo = owner();

        assert!(svc.list_of(yo, None, None).await.unwrap().is_empty());
        for err in [
            svc.get_of(yo, ajena.id).await.unwrap_err(),
            svc.update_of(yo, ajena.id, None, Some(true))
                .await
                .unwrap_err(),
            svc.remove_of(yo, ajena.id).await.unwrap_err(),
        ] {
            assert_eq!(err.status(), StatusCode::NOT_FOUND);
        }
        assert_eq!(
            svc.get_of(ajena.owner_id, ajena.id).await.unwrap().title,
            "ajena"
        );
    }
}
