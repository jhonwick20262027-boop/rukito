//! Controller de `files` (§8.2, RA-003, §26.13).
//!
//! Las tres rutas exigen sesión; el dueño y la clave salen de ella. El nombre y
//! el tipo llegan por cabecera y el cuerpo por el extractor `Bytes` (RF-278).

use rdd::controller;
use rdd::prelude::*;
use uuid::Uuid;

use crate::middleware::RequireAuth;
use crate::modules::files::FilesService;
use crate::modules::files::domain::{FileError, FileName};
use crate::modules::files::presentation::dto::FileResponse;
use crate::modules::users::Session;

pub struct FilesController;

#[controller("/files")]
#[middleware(RequireAuth)]
impl FilesController {
    /// `POST /files` — 201 con la metadata; el cuerpo es el archivo completo.
    #[post("/", status = 201)]
    async fn upload(
        svc: FilesService,
        session: Ext<Session>,
        x_file_name: Header<String>,
        content_type: Header<String>,
        body: Bytes,
    ) -> Result<FileResponse> {
        let name = FileName::parse(&x_file_name).map_err(domain_error)?;
        svc.upload_of(session.id, name, content_type.into_inner(), body)
            .await
            .map(FileResponse::from)
    }

    /// `GET /files/:id` — metadata del objeto propio; 404 si es de otro.
    #[get("/:id")]
    async fn meta(
        svc: FilesService,
        session: Ext<Session>,
        id: Path<Uuid>,
    ) -> Result<FileResponse> {
        svc.meta_of(session.id, *id).await.map(FileResponse::from)
    }

    /// `DELETE /files/:id` — 204 sin cuerpo.
    #[delete("/:id")]
    async fn remove(svc: FilesService, session: Ext<Session>, id: Path<Uuid>) -> Result<()> {
        svc.remove_of(session.id, *id).await
    }
}

fn domain_error(error: FileError) -> Error {
    match error {
        FileError::EmptyName => Error::bad_request("el nombre del archivo no puede estar vacío"),
        FileError::InvalidChar => {
            Error::bad_request("el nombre del archivo no puede tener `/`, `\\` ni controles")
        }
        FileError::NameTooLong { max } => Error::invalid(format!(
            "el nombre del archivo no puede pasar de {max} bytes"
        )),
    }
}
