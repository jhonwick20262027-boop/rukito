//! Documento OpenAPI 3.1 del demo (§10.10, §26.10).
//!
//! Es la raíz de composición de la documentación: conoce las rutas que monta
//! `main` y sus DTOs, y no reutiliza los handlers. `AxumPlatform::openapi`
//! valida que este documento cubra exactamente las rutas montadas (RF-248).

use rdd::openapi::{OpenApi, OpenApiBuilder, Operation, Parameter, Server, Tag};
use serde_json::{Value, json};

fn reference(name: &str) -> Value {
    json!({ "$ref": format!("#/components/schemas/{name}") })
}

fn array_of(name: &str) -> Value {
    json!({ "type": "array", "items": reference(name) })
}

pub fn document() -> OpenApi {
    OpenApiBuilder::new("todo-app", env!("CARGO_PKG_VERSION"))
        .description(
            "Demo de rdd: TODO con JWT, roles, CORS, rate limit, GraphQL y MCP sobre \
             el mismo contenedor (§26).",
        )
        .server(Server::new("/").description("Este servidor"))
        .tag(Tag::new("app").description("Bienvenida y estado del servicio"))
        .tag(Tag::new("auth").description("Registro, login y cuenta de la sesión"))
        .tag(Tag::new("tasks").description("Tareas del dueño del token"))
        .schema(
            "Problem",
            json!({
                "type": "object",
                "description": "Problem Details (RFC 9457)",
                "required": ["type", "title", "status"],
                "properties": {
                    "type": { "type": "string" },
                    "title": { "type": "string" },
                    "status": { "type": "integer" },
                    "detail": { "type": "string" },
                    "instance": { "type": "string" }
                }
            }),
        )
        .schema(
            "User",
            json!({
                "type": "object",
                "required": ["id", "email", "role"],
                "properties": {
                    "id": { "type": "string", "format": "uuid" },
                    "email": { "type": "string", "format": "email" },
                    "role": { "type": "string", "enum": ["user", "admin"] }
                }
            }),
        )
        .schema(
            "Token",
            json!({
                "type": "object",
                "required": ["access_token", "token_type", "user"],
                "properties": {
                    "access_token": { "type": "string" },
                    "token_type": { "type": "string", "const": "Bearer" },
                    "user": reference("User")
                }
            }),
        )
        .schema(
            "Credentials",
            json!({
                "type": "object",
                "required": ["email", "password"],
                "properties": {
                    "email": { "type": "string", "format": "email" },
                    "password": { "type": "string", "format": "password", "minLength": 8 }
                }
            }),
        )
        .schema(
            "Task",
            json!({
                "type": "object",
                "required": ["id", "title", "done", "owner_id"],
                "properties": {
                    "id": { "type": "string", "format": "uuid" },
                    "title": { "type": "string" },
                    "done": { "type": "boolean" },
                    "owner_id": { "type": "string", "format": "uuid" }
                }
            }),
        )
        .schema(
            "TaskCreate",
            json!({
                "type": "object",
                "required": ["title"],
                "properties": {
                    "title": { "type": "string", "minLength": 1 },
                    "done": { "type": "boolean" }
                }
            }),
        )
        .schema(
            "TaskUpdate",
            json!({
                "type": "object",
                "properties": {
                    "title": { "type": "string", "minLength": 1 },
                    "done": { "type": "boolean" }
                }
            }),
        )
        .operation(
            "get",
            "/",
            Operation::new("index", "Bienvenida del servicio")
                .tag("app")
                .json_response(200, "Mensaje de bienvenida", json!({ "type": "string" })),
        )
        .operation(
            "get",
            "/health",
            Operation::new("health", "Estado del servicio")
                .tag("app")
                .json_response(
                    200,
                    "El servicio responde",
                    json!({
                        "type": "object",
                        "required": ["status"],
                        "properties": { "status": { "type": "string" } }
                    }),
                ),
        )
        .operation(
            "post",
            "/auth/register",
            Operation::new("register", "Crea una cuenta")
                .tag("auth")
                .description("El rol nunca lo elige el cliente: nace como `user`.")
                .json_body(reference("Credentials"))
                .json_response(201, "Cuenta creada", reference("User"))
                .json_response(409, "El correo ya está registrado", reference("Problem")),
        )
        .operation(
            "post",
            "/auth/login",
            Operation::new("login", "Obtiene un JWT")
                .tag("auth")
                .json_body(reference("Credentials"))
                .json_response(200, "Token y cuenta", reference("Token"))
                .json_response(401, "Credenciales inválidas", reference("Problem")),
        )
        .operation(
            "get",
            "/users/me",
            Operation::new("me", "Cuenta de la sesión")
                .tag("auth")
                .bearer()
                .json_response(200, "Cuenta autenticada", reference("User"))
                .json_response(401, "Falta el token o no es válido", reference("Problem")),
        )
        .operation(
            "get",
            "/tasks",
            Operation::new("listTasks", "Lista las tareas del dueño")
                .tag("tasks")
                .bearer()
                .parameter(
                    Parameter::query("q", json!({ "type": "string" }))
                        .description("Filtra por texto del título"),
                )
                .parameter(
                    Parameter::query("done", json!({ "type": "boolean" }))
                        .description("Filtra por estado"),
                )
                .json_response(200, "Tareas del dueño", array_of("Task"))
                .json_response(401, "Falta el token o no es válido", reference("Problem")),
        )
        .operation(
            "post",
            "/tasks",
            Operation::new("createTask", "Crea una tarea")
                .tag("tasks")
                .bearer()
                .description("El dueño sale de la sesión, nunca del cuerpo.")
                .json_body(reference("TaskCreate"))
                .json_response(201, "Tarea creada", reference("Task"))
                .json_response(401, "Falta el token o no es válido", reference("Problem")),
        )
        .operation(
            "get",
            "/tasks/{id}",
            Operation::new("getTask", "Obtiene una tarea propia")
                .tag("tasks")
                .bearer()
                .parameter(
                    Parameter::path("id", json!({ "type": "string", "format": "uuid" }))
                        .description("Identificador de la tarea"),
                )
                .json_response(200, "Tarea encontrada", reference("Task"))
                .json_response(401, "Falta el token o no es válido", reference("Problem"))
                .json_response(404, "No existe o es de otro dueño", reference("Problem")),
        )
        .operation(
            "put",
            "/tasks/{id}",
            Operation::new("updateTask", "Actualiza una tarea propia")
                .tag("tasks")
                .bearer()
                .parameter(
                    Parameter::path("id", json!({ "type": "string", "format": "uuid" }))
                        .description("Identificador de la tarea"),
                )
                .json_body(reference("TaskUpdate"))
                .json_response(200, "Tarea actualizada", reference("Task"))
                .json_response(401, "Falta el token o no es válido", reference("Problem"))
                .json_response(404, "No existe o es de otro dueño", reference("Problem")),
        )
        .operation(
            "delete",
            "/tasks/{id}",
            Operation::new("deleteTask", "Elimina una tarea propia")
                .tag("tasks")
                .bearer()
                .description("Exige rol `admin` además de ser dueño.")
                .parameter(
                    Parameter::path("id", json!({ "type": "string", "format": "uuid" }))
                        .description("Identificador de la tarea"),
                )
                .response(204, "Sin contenido")
                .json_response(401, "Falta el token o no es válido", reference("Problem"))
                .json_response(403, "La sesión no es `admin`", reference("Problem"))
                .json_response(404, "No existe o es de otro dueño", reference("Problem")),
        )
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn documenta_las_diez_rutas_del_demo() {
        let doc = document();
        doc.validate().expect("el documento debe ser válido");
        assert_eq!(doc.documented_routes().len(), 10);
    }
}
