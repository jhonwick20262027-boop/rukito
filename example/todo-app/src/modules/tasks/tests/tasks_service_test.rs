//! Reglas de negocio de `TasksService`, con un almacén aislado por test.

use std::sync::Arc;

use rdd::prelude::*;

use crate::modules::tasks::constants::tasks_constants::MAX_TITLE_LEN;
use crate::modules::tasks::dto::create_task_dto::CreateTaskDto;
use crate::modules::tasks::dto::query_task_dto::QueryTaskDto;
use crate::modules::tasks::dto::update_task_dto::UpdateTaskDto;
use crate::modules::tasks::entities::task_entity::Task;
use crate::modules::tasks::services::tasks_service::TasksService;

fn repo() -> Repo<Task> {
    Repo::new(Arc::new(MemoryBackend::new()))
}

fn dto(title: &str) -> CreateTaskDto {
    CreateTaskDto {
        title: title.to_owned(),
        done: None,
    }
}

#[tokio::test]
async fn crea_y_recupera() {
    let repo = repo();
    let created = TasksService::create(repo.clone(), dto("pan"))
        .await
        .unwrap();
    assert_eq!(
        TasksService::get(repo, created.id).await.unwrap().title,
        "pan"
    );
}

#[tokio::test]
async fn titulo_vacio_es_400() {
    let err = TasksService::create(repo(), dto("   ")).await.unwrap_err();
    assert_eq!(err.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn titulo_largo_es_422() {
    let err = TasksService::create(repo(), dto(&"x".repeat(MAX_TITLE_LEN + 1)))
        .await
        .unwrap_err();
    assert_eq!(err.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn id_inexistente_es_404() {
    let err = TasksService::get(repo(), uuid::Uuid::nil())
        .await
        .unwrap_err();
    assert_eq!(err.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn lista_filtra_por_texto() {
    let repo = repo();
    TasksService::create(repo.clone(), dto("pan"))
        .await
        .unwrap();
    TasksService::create(repo.clone(), dto("café"))
        .await
        .unwrap();
    let query = QueryTaskDto {
        q: Some("ca".to_owned()),
        done: None,
    };
    let found = TasksService::list(repo, query).await.unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].title, "café");
}

#[tokio::test]
async fn actualiza_y_borra() {
    let repo = repo();
    let created = TasksService::create(repo.clone(), dto("pan"))
        .await
        .unwrap();
    let changes = UpdateTaskDto {
        title: None,
        done: Some(true),
    };
    let updated = TasksService::update(repo.clone(), created.id, changes)
        .await
        .unwrap();
    assert!(updated.done);
    assert_eq!(updated.title, "pan");
    TasksService::remove(repo.clone(), created.id)
        .await
        .unwrap();
    let err = TasksService::get(repo, created.id).await.unwrap_err();
    assert_eq!(err.status(), StatusCode::NOT_FOUND);
}
