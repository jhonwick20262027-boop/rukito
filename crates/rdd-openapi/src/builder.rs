//! Builder del documento OpenAPI (§10.10).
//!
//! Encadena servidores, etiquetas, esquemas y operaciones; `build` devuelve el
//! documento que el montaje valida y sirve.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::model::{Components, Info, OpenApi, Operation, Server, Tag};

/// Constructor del documento OpenAPI 3.1.
pub struct OpenApiBuilder {
    doc: OpenApi,
}

impl OpenApiBuilder {
    /// Documento vacío con `info` y el esquema `bearerAuth` declarado.
    pub fn new(title: impl Into<String>, version: impl Into<String>) -> Self {
        let mut security_schemes = BTreeMap::new();
        security_schemes.insert(
            crate::BEARER_SCHEME.to_owned(),
            json!({
                "type": "http",
                "scheme": "bearer",
                "bearerFormat": "JWT",
                "description": "Token JWT en `Authorization: Bearer <token>`"
            }),
        );
        Self {
            doc: OpenApi {
                openapi: "3.1.0".to_owned(),
                info: Info::new(title, version),
                servers: Vec::new(),
                tags: Vec::new(),
                paths: BTreeMap::new(),
                components: Components {
                    schemas: BTreeMap::new(),
                    security_schemes,
                },
            },
        }
    }

    /// Descripción de la API mostrada en la UI.
    pub fn description(mut self, text: impl Into<String>) -> Self {
        self.doc.info.description = Some(text.into());
        self
    }

    /// Servidor donde la UI envía las pruebas.
    pub fn server(mut self, server: Server) -> Self {
        self.doc.servers.push(server);
        self
    }

    /// Etiqueta que agrupa operaciones.
    pub fn tag(mut self, tag: Tag) -> Self {
        self.doc.tags.push(tag);
        self
    }

    /// Esquema reutilizable en `components.schemas`.
    pub fn schema(mut self, name: impl Into<String>, schema: Value) -> Self {
        self.doc.components.schemas.insert(name.into(), schema);
        self
    }

    /// Registra una operación; el método se guarda en minúsculas.
    ///
    /// Si dos operaciones declaran el mismo método y ruta, la última gana; el
    /// choque real contra las rutas montadas lo detecta `check_routes` (RF-248).
    pub fn operation(
        mut self,
        method: impl Into<String>,
        path: impl Into<String>,
        operation: Operation,
    ) -> Self {
        self.doc
            .paths
            .entry(path.into())
            .or_default()
            .insert(method.into().to_lowercase(), operation);
        self
    }

    /// Documento listo para validar y montar.
    pub fn build(self) -> OpenApi {
        self.doc
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agrupa_operaciones_por_ruta_y_metodo() {
        let doc = OpenApiBuilder::new("demo", "1.0")
            .server(Server::new("http://localhost:3000"))
            .tag(Tag::new("tasks"))
            .operation(
                "GET",
                "/tasks",
                Operation::new("list", "Lista").response(200, "ok"),
            )
            .operation(
                "POST",
                "/tasks",
                Operation::new("create", "Crea").response(201, "ok"),
            )
            .build();

        assert_eq!(doc.paths.len(), 1);
        assert!(doc.paths["/tasks"].contains_key("get"));
        assert!(doc.paths["/tasks"].contains_key("post"));
        assert!(
            doc.components
                .security_schemes
                .contains_key(crate::BEARER_SCHEME)
        );
    }
}
