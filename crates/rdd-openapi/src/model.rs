//! Modelo del documento OpenAPI 3.1 (§10.10).
//!
//! Solo se modela el subconjunto que el módulo publica y la UI consume: no hay
//! `$ref` propios, callbacks ni enlaces. Los esquemas viajan como `serde_json`
//! para no inventar un segundo sistema de tipos.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::SecurityRequirement;

/// Métodos que el documento admite, en minúsculas como exige OpenAPI.
const METHODS: [&str; 8] = [
    "get", "put", "post", "delete", "options", "head", "patch", "trace",
];

/// Documento OpenAPI 3.1 (§10.10).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenApi {
    /// Versión del documento; el módulo emite `3.1.0`.
    pub openapi: String,
    /// Título, versión y descripción de la API.
    pub info: Info,
    /// Servidores donde la UI prueba las operaciones.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub servers: Vec<Server>,
    /// Etiquetas que agrupan operaciones.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<Tag>,
    /// Rutas: `{ "/tasks": { "get": Operation, "post": Operation } }`.
    pub paths: BTreeMap<String, PathItem>,
    /// Esquemas y esquemas de seguridad.
    #[serde(default)]
    pub components: Components,
}

/// Operaciones de una ruta, indexadas por método en minúsculas.
pub type PathItem = BTreeMap<String, Operation>;

impl OpenApi {
    /// Valida la forma del documento antes de servirlo (RF-247).
    pub fn validate(&self) -> anyhow::Result<()> {
        if !self.openapi.starts_with("3.") {
            return Err(anyhow::anyhow!(
                "`openapi` debe declarar una versión 3.x: {:?}",
                self.openapi
            ));
        }
        if self.info.title.trim().is_empty() || self.info.version.trim().is_empty() {
            return Err(anyhow::anyhow!(
                "`info.title` e `info.version` no pueden estar vacíos"
            ));
        }
        let mut ids = BTreeSet::new();
        for (path, item) in &self.paths {
            if !path.starts_with('/') {
                return Err(anyhow::anyhow!("la ruta `{path}` debe empezar por `/`"));
            }
            if item.is_empty() {
                return Err(anyhow::anyhow!(
                    "la ruta `{path}` no declara ninguna operación"
                ));
            }
            for (method, operation) in item {
                if !METHODS.contains(&method.as_str()) {
                    return Err(anyhow::anyhow!("método `{method}` no válido en `{path}`"));
                }
                if operation.responses.is_empty() {
                    return Err(anyhow::anyhow!(
                        "`{} {}` no declara respuestas",
                        method.to_uppercase(),
                        path
                    ));
                }
                if let Some(id) = &operation.operation_id
                    && !ids.insert(id.clone())
                {
                    return Err(anyhow::anyhow!("`operationId` duplicado: `{id}`"));
                }
            }
        }
        Ok(())
    }

    /// Operaciones documentadas como `(MÉTODO, ruta)`.
    pub fn documented_routes(&self) -> BTreeSet<(String, String)> {
        let mut routes = BTreeSet::new();
        for (path, item) in &self.paths {
            for method in item.keys() {
                routes.insert((method.to_uppercase(), core_style(path)));
            }
        }
        routes
    }

    /// Compara el documento con las rutas montadas por el `HttpApp` (RF-248).
    ///
    /// Con `strict` en `true` las dos direcciones importan: una ruta montada
    /// sin documentar y una documentada que no existe impiden arrancar.
    pub fn check_routes<I>(&self, routes: I, strict: bool) -> anyhow::Result<()>
    where
        I: IntoIterator<Item = (String, String)>,
    {
        let mounted: BTreeSet<(String, String)> = routes.into_iter().collect();
        let documented = self.documented_routes();
        let missing: Vec<String> = mounted
            .difference(&documented)
            .map(|(method, path)| format!("{method} {path}"))
            .collect();
        let extra: Vec<String> = documented
            .difference(&mounted)
            .map(|(method, path)| format!("{method} {path}"))
            .collect();
        if strict && !missing.is_empty() {
            return Err(anyhow::anyhow!(
                "el documento OpenAPI no cubre rutas montadas: {}",
                missing.join(", ")
            ));
        }
        if !extra.is_empty() {
            return Err(anyhow::anyhow!(
                "el documento OpenAPI declara rutas inexistentes: {}",
                extra.join(", ")
            ));
        }
        Ok(())
    }
}

/// Convierte `/tasks/{id}` a la sintaxis del núcleo `/tasks/:id`.
fn core_style(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    let mut chars = path.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '{' {
            out.push(':');
            for inner in chars.by_ref() {
                if inner == '}' {
                    break;
                }
                out.push(inner);
            }
        } else {
            out.push(ch);
        }
    }
    out
}

/// Versión, título y descripción de la API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Info {
    /// Nombre de la API.
    pub title: String,
    /// Versión publicada.
    pub version: String,
    /// Texto libre para la UI.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl Info {
    /// Título y versión, como exige OpenAPI.
    pub fn new(title: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            version: version.into(),
            description: None,
        }
    }

    /// Descripción de la API.
    pub fn description(mut self, text: impl Into<String>) -> Self {
        self.description = Some(text.into());
        self
    }
}

/// Servidor donde la UI envía las pruebas.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Server {
    /// URL base, absoluta o relativa.
    pub url: String,
    /// Etiqueta legible.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl Server {
    /// Servidor con su URL.
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            description: None,
        }
    }

    /// Etiqueta del servidor.
    pub fn description(mut self, text: impl Into<String>) -> Self {
        self.description = Some(text.into());
        self
    }
}

/// Etiqueta que agrupa operaciones en la UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tag {
    /// Identificador de la etiqueta.
    pub name: String,
    /// Descripción mostrada en el encabezado del grupo.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl Tag {
    /// Etiqueta por nombre.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: None,
        }
    }

    /// Descripción de la etiqueta.
    pub fn description(mut self, text: impl Into<String>) -> Self {
        self.description = Some(text.into());
        self
    }
}

/// Operación HTTP documentada.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Operation {
    /// Etiquetas a las que pertenece.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// Identificador único de la operación.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<String>,
    /// Resumen corto.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// Descripción larga.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Parámetros de ruta, query o cabecera.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parameters: Vec<Parameter>,
    /// Cuerpo de la petición.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_body: Option<RequestBody>,
    /// Respuestas por código de estado.
    pub responses: BTreeMap<String, ApiResponse>,
    /// Requisitos de seguridad.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub security: Vec<SecurityRequirement>,
    /// Marca la operación como obsoleta.
    #[serde(default, skip_serializing_if = "is_false")]
    pub deprecated: bool,
}

impl Operation {
    /// Operación con identificador y resumen.
    pub fn new(operation_id: impl Into<String>, summary: impl Into<String>) -> Self {
        Self {
            operation_id: Some(operation_id.into()),
            summary: Some(summary.into()),
            ..Self::default()
        }
    }

    /// Añade la operación a una etiqueta.
    pub fn tag(mut self, tag: impl Into<String>) -> Self {
        self.tags.push(tag.into());
        self
    }

    /// Descripción larga.
    pub fn description(mut self, text: impl Into<String>) -> Self {
        self.description = Some(text.into());
        self
    }

    /// Añade un parámetro.
    pub fn parameter(mut self, parameter: Parameter) -> Self {
        self.parameters.push(parameter);
        self
    }

    /// Cuerpo JSON con su esquema.
    pub fn json_body(mut self, schema: Value) -> Self {
        self.request_body = Some(RequestBody {
            description: None,
            required: true,
            content: BTreeMap::from([(
                "application/json".to_owned(),
                MediaType {
                    schema: Some(schema),
                    example: None,
                },
            )]),
        });
        self
    }

    /// Respuesta sin cuerpo.
    pub fn response(mut self, status: u16, description: impl Into<String>) -> Self {
        self.responses.insert(
            status.to_string(),
            ApiResponse {
                description: description.into(),
                content: BTreeMap::new(),
            },
        );
        self
    }

    /// Respuesta JSON con su esquema.
    pub fn json_response(
        mut self,
        status: u16,
        description: impl Into<String>,
        schema: Value,
    ) -> Self {
        self.responses.insert(
            status.to_string(),
            ApiResponse {
                description: description.into(),
                content: BTreeMap::from([(
                    "application/json".to_owned(),
                    MediaType {
                        schema: Some(schema),
                        example: None,
                    },
                )]),
            },
        );
        self
    }

    /// Exige el `Bearer` de `bearerAuth` (RF-247).
    pub fn bearer(mut self) -> Self {
        self.security.push(BTreeMap::from([(
            crate::BEARER_SCHEME.to_owned(),
            Vec::new(),
        )]));
        self
    }

    /// Marca la operación como obsoleta.
    pub fn deprecated(mut self) -> Self {
        self.deprecated = true;
        self
    }
}

/// Parámetro de una operación.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Parameter {
    /// Nombre del parámetro.
    pub name: String,
    /// Dónde viaja.
    #[serde(rename = "in")]
    pub location: ParameterLocation,
    /// Descripción mostrada en la UI.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Obligatorio en la petición.
    #[serde(default)]
    pub required: bool,
    /// Esquema JSON del valor.
    pub schema: Value,
    /// Ejemplo para la UI.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub example: Option<Value>,
}

impl Parameter {
    /// Parámetro de ruta (siempre obligatorio).
    pub fn path(name: impl Into<String>, schema: Value) -> Self {
        Self {
            name: name.into(),
            location: ParameterLocation::Path,
            description: None,
            required: true,
            schema,
            example: None,
        }
    }

    /// Parámetro de query.
    pub fn query(name: impl Into<String>, schema: Value) -> Self {
        Self {
            name: name.into(),
            location: ParameterLocation::Query,
            description: None,
            required: false,
            schema,
            example: None,
        }
    }

    /// Parámetro de cabecera.
    pub fn header(name: impl Into<String>, schema: Value) -> Self {
        Self {
            name: name.into(),
            location: ParameterLocation::Header,
            description: None,
            required: false,
            schema,
            example: None,
        }
    }

    /// Descripción del parámetro.
    pub fn description(mut self, text: impl Into<String>) -> Self {
        self.description = Some(text.into());
        self
    }

    /// Marca el parámetro como obligatorio.
    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }
}

/// Ubicación de un parámetro.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ParameterLocation {
    /// Segmento de la ruta.
    Path,
    /// Cadena de consulta.
    Query,
    /// Cabecera HTTP.
    Header,
}

/// Cuerpo de la petición.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestBody {
    /// Descripción del cuerpo.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Si el cuerpo es obligatorio.
    #[serde(default)]
    pub required: bool,
    /// Contenido por tipo MIME.
    pub content: BTreeMap<String, MediaType>,
}

/// Respuesta documentada.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiResponse {
    /// Descripción del código de estado.
    pub description: String,
    /// Contenido por tipo MIME.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub content: BTreeMap<String, MediaType>,
}

/// Cuerpo y esquema de un tipo MIME.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaType {
    /// Esquema JSON del cuerpo.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<Value>,
    /// Ejemplo ya resuelto.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub example: Option<Value>,
}

/// Esquemas reutilizables y esquemas de seguridad.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Components {
    /// Esquemas con nombre.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub schemas: BTreeMap<String, Value>,
    /// Esquemas de seguridad, p. ej. `bearerAuth`.
    #[serde(
        default,
        rename = "securitySchemes",
        skip_serializing_if = "BTreeMap::is_empty"
    )]
    pub security_schemes: BTreeMap<String, Value>,
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc() -> OpenApi {
        crate::OpenApiBuilder::new("demo", "1.0")
            .operation(
                "get",
                "/tasks/{id}",
                Operation::new("getTask", "Una tarea").response(200, "ok"),
            )
            .build()
    }

    #[test]
    fn el_documento_valida_y_compara_rutas() {
        let doc = doc();
        doc.validate().unwrap();
        doc.check_routes([("GET".to_owned(), "/tasks/:id".to_owned())], true)
            .unwrap();
    }

    #[test]
    fn una_ruta_sin_documentar_falla() {
        let err = doc()
            .check_routes(
                [
                    ("GET".to_owned(), "/tasks/:id".to_owned()),
                    ("GET".to_owned(), "/health".to_owned()),
                ],
                true,
            )
            .unwrap_err();
        assert!(err.to_string().contains("GET /health"));
    }

    #[test]
    fn una_ruta_documentada_que_no_existe_falla() {
        let err = doc()
            .check_routes([("GET".to_owned(), "/otra".to_owned())], false)
            .unwrap_err();
        assert!(err.to_string().contains("GET /tasks/:id"));
    }

    #[test]
    fn la_validacion_rechaza_un_documento_mal_formado() {
        let mut sin_barra = doc();
        let item = sin_barra.paths.remove("/tasks/{id}").unwrap();
        sin_barra.paths.insert("tasks/{id}".to_owned(), item);
        assert!(
            sin_barra
                .validate()
                .unwrap_err()
                .to_string()
                .contains("empezar por `/`")
        );

        let mut sin_respuestas = doc();
        sin_respuestas
            .paths
            .get_mut("/tasks/{id}")
            .unwrap()
            .insert("get".to_owned(), Operation::new("otra", "Sin respuestas"));
        assert!(
            sin_respuestas
                .validate()
                .unwrap_err()
                .to_string()
                .contains("no declara respuestas")
        );

        let mut id_duplicado = doc();
        id_duplicado.paths.get_mut("/tasks/{id}").unwrap().insert(
            "put".to_owned(),
            Operation::new("getTask", "Mismo id").response(200, "ok"),
        );
        assert!(
            id_duplicado
                .validate()
                .unwrap_err()
                .to_string()
                .contains("operationId")
        );

        let mut version_mala = doc();
        version_mala.openapi = "2.0".to_owned();
        assert!(version_mala.validate().is_err());
    }
}
