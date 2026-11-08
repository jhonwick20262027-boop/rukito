# TODO de ejemplo con SQLite

Proyecto generado con `rdd new todo-app` y adaptado para persistir tareas usando
el `Repo<Task>` del framework y el adaptador SQLite predeterminado.
Requiere Rust 1.94 o posterior.

## Ejecutar

Instala el CLI desde la raíz del workspace una vez:

```sh
cargo install --path crates/rdd-cli
```

La app crea `todo_app.db` si no existe y escucha en `http://localhost:3000`.
Antes de iniciar o desplegarla, aplica las migraciones pendientes:

```sh
cd example/todo-app
rdd migrate
cargo run
```

`rdd migrate` es idempotente y registra las versiones aplicadas en
`_sqlx_migrations`; no se ejecuta implícitamente al arrancar el servidor. Para
ejecutar con el backend en memoria, usa `rdd run --memory` o define
`RDD_MEMORY=1`.

## Linux y macOS

Se requiere Rust 1.94 o posterior y un compilador C para compilar SQLite
bundled: `build-essential` en Debian/Ubuntu o Xcode Command Line Tools en macOS
(`xcode-select --install`). No hace falta instalar un servidor ni las librerías
de desarrollo de SQLite. Los mismos comandos de migración y ejecución aplican
en Linux, macOS y Windows.

## API

- `GET /tasks` — lista; acepta `?q=texto` y `?done=true|false`.
- `GET /tasks/{id}` — obtiene una tarea por UUID.
- `POST /tasks` — crea con `{"title":"Comprar leche"}`.
- `PUT /tasks/{id}` — actualiza parcialmente, por ejemplo `{"done":true}`.
- `DELETE /tasks/{id}` — elimina la tarea.

Ejecuta `cargo test` para verificar el CRUD con el backend en memoria y con
SQLite en memoria.
