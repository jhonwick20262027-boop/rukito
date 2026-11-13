//! Módulo `files` (§26.13, RF-277).
//!
//! Raíz de composición: declara las cuatro capas y fija el adaptador del caso de
//! uso. El storage llega construido desde `config/upload_config.rs`, igual que
//! el codec de `users`, para que la suite monte uno en memoria sin tocar el
//! entorno (RF-160).

pub mod application;
pub mod domain;
pub mod infrastructure;
pub mod presentation;

use std::sync::Arc;

use rdd::module;
use rdd::prelude::*;
use rdd::upload::ObjectStorage;

use self::application::files_use_cases::FilesUseCases;
use self::infrastructure::file_store::FileStore;
use self::presentation::files_controller::FilesController;

/// El caso de uso con el adaptador del contenedor. La capa HTTP importa este
/// alias y nunca el envoltorio de infraestructura (RA-003, §14.2).
pub type FilesService = FilesUseCases<FileStore>;

impl FromContainer for FilesService {
    const NAME: &'static str = "FilesService";

    fn from_container(container: &Container) -> Result<Self> {
        let storage = container.get::<Arc<dyn ObjectStorage>>().ok_or_else(|| {
            Error::not_found("falta el storage de `files`: monta `files::module(storage)` (§26.13)")
        })?;
        Ok(Self::with_storage(FileStore::new(storage)))
    }
}

impl FromRequest for FilesService {
    async fn from_request(ctx: &Ctx, _bind: Bind) -> Result<Self> {
        ctx.resolve::<Self>()
    }
}

/// El módulo listo para montar, con el storage elegido por `UPLOAD_DRIVER`.
pub fn module(storage: Arc<dyn ObjectStorage>) -> rdd::Module {
    module! {
        providers: [],
        controllers: [FilesController],
        middlewares: [],
    }
    .provider(ProviderReg::new(Vec::new(), move |_| {
        Ok::<_, Error>(storage.clone())
    }))
}

#[cfg(test)]
mod tests {
    use rdd::upload::MemoryStorage;

    use super::*;
    use crate::modules::files::domain::FileName;

    fn service() -> FilesUseCases<MemoryStorage> {
        FilesUseCases::with_storage(MemoryStorage::new())
    }

    #[tokio::test]
    async fn sube_y_recupera_la_metadata() {
        let svc = service();
        let owner = uuid::Uuid::new_v4();
        let created = svc
            .upload_of(
                owner,
                FileName::parse("notas.txt").unwrap(),
                "text/plain".to_owned(),
                Bytes::from_static(b"hola"),
            )
            .await
            .unwrap();
        assert_eq!(created.size, 4);
        assert_eq!(created.content_type.as_deref(), Some("text/plain"));

        let meta = svc.meta_of(owner, created.id).await.unwrap();
        assert_eq!(meta, created);

        svc.remove_of(owner, created.id).await.unwrap();
        assert_eq!(
            svc.meta_of(owner, created.id).await.unwrap_err().status(),
            StatusCode::NOT_FOUND
        );
    }

    #[tokio::test]
    async fn el_archivo_de_otro_es_404() {
        let svc = service();
        let ajena = svc
            .upload_of(
                uuid::Uuid::new_v4(),
                FileName::parse("ajena.txt").unwrap(),
                "text/plain".to_owned(),
                Bytes::from_static(b"x"),
            )
            .await
            .unwrap();
        let yo = uuid::Uuid::new_v4();

        for err in [
            svc.meta_of(yo, ajena.id).await.unwrap_err(),
            svc.remove_of(yo, ajena.id).await.unwrap_err(),
        ] {
            assert_eq!(err.status(), StatusCode::NOT_FOUND);
        }
    }
}
