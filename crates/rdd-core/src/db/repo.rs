//! `Repo<T>`: el handle de persistencia que ven los services (§12.1).

use std::marker::PhantomData;
use std::sync::Arc;

use crate::db::{Db, DbAdapter, Persist, Value};
use crate::di::{Container, FromContainer};
use crate::domain::Aggregate;
use crate::error::{Error, Result};

/// Repositorio de un agregado sobre cualquier motor.
///
/// Es un handle clonable y barato: solo guarda el `Arc` del adaptador (RF-110).
/// Cambiar de PostgreSQL a MongoDB no toca este tipo ni el service (RF-185).
pub struct Repo<T: Aggregate + Persist> {
    adapter: Arc<dyn DbAdapter>,
    _marker: PhantomData<fn() -> T>,
}

impl<T: Aggregate + Persist> Clone for Repo<T> {
    fn clone(&self) -> Self {
        Self {
            adapter: Arc::clone(&self.adapter),
            _marker: PhantomData,
        }
    }
}

impl<T: Aggregate + Persist> std::fmt::Debug for Repo<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Repo")
            .field("table", &T::TABLE)
            .field("adapter", &self.adapter.name())
            .finish()
    }
}

impl<T: Aggregate + Persist> Repo<T>
where
    T::Id: Into<Value>,
{
    /// Handle sobre un adaptador ya conectado.
    pub fn new(adapter: Arc<dyn DbAdapter>) -> Self {
        Self {
            adapter,
            _marker: PhantomData,
        }
    }

    /// Tabla, colección o prefijo del agregado.
    pub fn table(&self) -> &'static str {
        T::TABLE
    }

    /// El adaptador activo, para los adaptadores y la suite de conformidad.
    pub fn adapter(&self) -> &Arc<dyn DbAdapter> {
        &self.adapter
    }

    /// `COLUMNS[0]` es el identificador (§12.2). Se comprueba en vez de indexar a
    /// ciegas: un `COLUMNS` vacío es un error de esquema, no un panic en petición.
    fn id_column() -> Result<&'static str> {
        T::COLUMNS.first().copied().ok_or_else(|| {
            Error::internal(anyhow::anyhow!(
                "`{}::COLUMNS` está vacío: la primera columna debe ser el identificador (§12.2)",
                T::TABLE
            ))
        })
    }

    /// El agregado, o `NotFound` si no existe.
    pub async fn get(&self, id: T::Id) -> Result<T> {
        let id: Value = id.into();
        let key = id.key();
        match self.adapter.get(T::TABLE, Self::id_column()?, &id).await? {
            Some(row) => T::from_row(&row),
            None => Err(not_found(T::TABLE, &key)),
        }
    }

    /// El agregado, o `None` si no existe.
    pub async fn find(&self, id: T::Id) -> Result<Option<T>> {
        let id: Value = id.into();
        match self.adapter.get(T::TABLE, Self::id_column()?, &id).await? {
            Some(row) => T::from_row(&row).map(Some),
            None => Ok(None),
        }
    }

    /// Todos los agregados de la tabla.
    pub async fn all(&self) -> Result<Vec<T>> {
        self.adapter
            .all(T::TABLE)
            .await?
            .iter()
            .map(T::from_row)
            .collect()
    }

    /// Upsert por identificador (RF-112).
    pub async fn save(&self, aggregate: &T) -> Result<()> {
        let values = aggregate.to_values();
        if values.len() != T::COLUMNS.len() {
            return Err(Error::internal(anyhow::anyhow!(
                "`{}::to_values` devolvió {} valores para {} columnas: deben coincidir (§12.2)",
                T::TABLE,
                values.len(),
                T::COLUMNS.len()
            )));
        }
        self.adapter.save(T::TABLE, T::COLUMNS, &values).await
    }

    /// Borra por identificador, o `NotFound` si no existía.
    pub async fn delete(&self, id: T::Id) -> Result<()> {
        let id: Value = id.into();
        let key = id.key();
        if self
            .adapter
            .delete(T::TABLE, Self::id_column()?, &id)
            .await?
        {
            Ok(())
        } else {
            Err(not_found(T::TABLE, &key))
        }
    }

    /// Cierto si el identificador existe. Se resuelve con `get` (§12.1).
    pub async fn exists(&self, id: T::Id) -> Result<bool> {
        let id: Value = id.into();
        Ok(self
            .adapter
            .get(T::TABLE, Self::id_column()?, &id)
            .await?
            .is_some())
    }
}

impl<T: Aggregate + Persist> FromContainer for Repo<T>
where
    T::Id: Into<Value>,
{
    const NAME: &'static str = "Repo";

    fn from_container(container: &Container) -> Result<Self> {
        let db = container.get::<Db>().ok_or_else(|| {
            Error::not_found(
                "no hay adaptador de persistencia: usa `.database(url)`, `.adapter(...)` \
                 o `.in_memory()` en App (§12.6)",
            )
        })?;
        Ok(Self::new(db.0))
    }
}

/// RF-040: el 404 nombra la tabla y el identificador, no el motor.
fn not_found(table: &str, key: &str) -> Error {
    Error::not_found(format!("no existe una fila de `{table}` con id {key}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{MemoryBackend, Row};

    #[derive(Clone, Debug, PartialEq)]
    struct Task {
        id: i64,
        title: String,
        done: bool,
    }

    impl Aggregate for Task {
        type Id = i64;

        fn id(&self) -> &Self::Id {
            &self.id
        }
    }

    impl Persist for Task {
        const TABLE: &'static str = "tasks";
        const COLUMNS: &'static [&'static str] = &["id", "title", "done"];

        fn to_values(&self) -> Vec<Value> {
            vec![
                Value::I64(self.id),
                Value::from(self.title.clone()),
                Value::Bool(self.done),
            ]
        }

        fn from_row(row: &Row) -> Result<Self> {
            Ok(Self {
                id: row.value("id")?.as_i64()?,
                title: row.value("title")?.as_text()?.to_owned(),
                done: row.value("done")?.as_bool()?,
            })
        }
    }

    fn repo() -> Repo<Task> {
        Repo::new(Arc::new(MemoryBackend::new()))
    }

    fn task(id: i64, title: &str) -> Task {
        Task {
            id,
            title: title.to_owned(),
            done: false,
        }
    }

    #[tokio::test]
    async fn round_trip_de_un_agregado() {
        let repo = repo();
        let original = task(1, "escribir el puerto");
        repo.save(&original).await.unwrap();
        assert_eq!(repo.get(1).await.unwrap(), original);
        assert_eq!(repo.all().await.unwrap(), vec![original]);
    }

    #[tokio::test]
    async fn save_actualiza_sin_duplicar() {
        let repo = repo();
        repo.save(&task(1, "uno")).await.unwrap();
        let mut changed = task(1, "dos");
        changed.done = true;
        repo.save(&changed).await.unwrap();
        assert_eq!(repo.all().await.unwrap().len(), 1);
        assert_eq!(repo.get(1).await.unwrap(), changed);
    }

    #[tokio::test]
    async fn get_y_delete_de_un_id_ausente_son_not_found() {
        let repo = repo();
        let err = repo.get(7).await.unwrap_err();
        assert_eq!(err.status(), crate::http::StatusCode::NOT_FOUND);
        assert!(err.to_string().contains("tasks"), "{err}");
        let err = repo.delete(7).await.unwrap_err();
        assert_eq!(err.status(), crate::http::StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn find_y_exists_no_fallan_con_un_id_ausente() {
        let repo = repo();
        assert!(repo.find(7).await.unwrap().is_none());
        assert!(!repo.exists(7).await.unwrap());
        repo.save(&task(7, "siete")).await.unwrap();
        assert!(repo.exists(7).await.unwrap());
        assert!(repo.find(7).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn all_de_una_tabla_vacia_es_vec_vacio() {
        assert!(repo().all().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn delete_borra_la_fila() {
        let repo = repo();
        repo.save(&task(1, "uno")).await.unwrap();
        repo.delete(1).await.unwrap();
        assert!(repo.all().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn se_inyecta_desde_el_contenedor() {
        let mut builder = crate::di::ContainerBuilder::new();
        builder.provide(Db(Arc::new(MemoryBackend::new())));
        let container = builder.build().unwrap();
        let repo = container
            .resolve::<Repo<Task>>("TaskService", "repo")
            .unwrap();
        repo.save(&task(1, "uno")).await.unwrap();
        assert_eq!(repo.table(), "tasks");
    }

    #[tokio::test]
    async fn sin_adaptador_el_contenedor_nombra_el_campo() {
        let container = Container::empty();
        let err = container
            .resolve::<Repo<Task>>("TaskService", "repo")
            .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("TaskService"), "{msg}");
        assert!(msg.contains("repo"), "{msg}");
    }
}
