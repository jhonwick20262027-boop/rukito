//! Caso bueno de GraphQL (§10.9, RF-240): macros, esquema, ejecución e
//! introspección sobre el motor real.
//!
//! Una prueba `pass` de trybuild compila **y ejecuta** el archivo, así que aquí
//! se verifica también lo que produce la expansión: nombres en `camelCase`,
//! `#[graphql(skip)]`, doc comments, variables de entrada y raíces.

use rdd::graphql::{
    GraphqlContext, GraphqlInput, GraphqlObject, GraphqlRequest, GraphqlStream, Schema,
    graphql_object,
};

/// Una tarea publicada en el esquema.
#[derive(GraphqlObject)]
pub struct Task {
    /// Identificador.
    pub id: uuid::Uuid,
    pub title: String,
    pub done: bool,
    #[graphql(skip)]
    pub owner_id: uuid::Uuid,
    #[graphql(name = "dueAt")]
    pub due_at: Option<String>,
}

#[derive(GraphqlInput)]
pub struct CreateTaskInput {
    pub title: String,
    pub done: Option<bool>,
}

#[derive(Default)]
pub struct Query;

#[graphql_object(query)]
impl Query {
    /// Tareas del usuario.
    async fn tasks(&self, _ctx: &GraphqlContext) -> rdd::Result<Vec<Task>> {
        Ok(vec![Task {
            id: uuid::Uuid::nil(),
            title: "demo".to_owned(),
            done: false,
            owner_id: uuid::Uuid::nil(),
            due_at: None,
        }])
    }

    async fn task(
        &self,
        _ctx: &GraphqlContext,
        id: uuid::Uuid,
    ) -> rdd::Result<Option<Task>> {
        let _ = id;
        Ok(None)
    }
}

#[derive(Default)]
pub struct Mutation;

#[graphql_object(mutation)]
impl Mutation {
    async fn create_task(
        &self,
        _ctx: &GraphqlContext,
        input: CreateTaskInput,
    ) -> rdd::Result<Task> {
        Ok(Task {
            id: uuid::Uuid::nil(),
            title: input.title,
            done: input.done.unwrap_or(false),
            owner_id: uuid::Uuid::nil(),
            due_at: None,
        })
    }
}

#[derive(Default)]
pub struct Subscription;

#[graphql_object(subscription)]
impl Subscription {
    /// Cambios de tareas.
    async fn task_changed(
        &self,
        _ctx: &GraphqlContext,
    ) -> rdd::Result<GraphqlStream<Task>> {
        let (_tx, rx) = tokio::sync::mpsc::channel(1);
        Ok(GraphqlStream::new(rx))
    }
}

#[tokio::main]
async fn main() {
    let schema = Schema::builder()
        .query(Query)
        .mutation(Mutation)
        .subscription(Subscription)
        .build()
        .expect("esquema válido");
    let ctx = GraphqlContext::empty();

    let query = schema
        .execute(
            &ctx,
            GraphqlRequest::new("{ tasks { id title done dueAt __typename } }"),
        )
        .await;
    assert_eq!(query["data"]["tasks"][0]["title"], "demo");
    assert_eq!(query["data"]["tasks"][0]["done"], false);
    assert_eq!(query["data"]["tasks"][0]["__typename"], "Task");
    assert_eq!(query["data"]["tasks"][0]["dueAt"], serde_json::Value::Null);
    assert!(query["data"]["tasks"][0].get("ownerId").is_none());

    let mutation = schema
        .execute(
            &ctx,
            GraphqlRequest {
                query: "mutation($input: CreateTaskInput!) { createTask(input: $input) { title done } }"
                    .to_owned(),
                operation_name: None,
                variables: Some(serde_json::json!({ "input": { "title": "nueva" } })),
            },
        )
        .await;
    assert_eq!(mutation["data"]["createTask"]["title"], "nueva");
    assert_eq!(mutation["data"]["createTask"]["done"], false);

    let introspection = schema
        .execute(
            &ctx,
            GraphqlRequest::new("{ __schema { queryType { name } } }"),
        )
        .await;
    assert_eq!(introspection["data"]["__schema"]["queryType"]["name"], "Query");

    let unknown = schema
        .execute(&ctx, GraphqlRequest::new("{ tasks { titel } }"))
        .await;
    assert_eq!(unknown["data"], serde_json::Value::Null);
    assert_eq!(unknown["errors"][0]["path"][0], "tasks");
}
