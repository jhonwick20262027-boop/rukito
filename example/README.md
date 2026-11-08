# Ejemplos

Cada aplicación de ejemplo vive en su propia carpeta:

- [todo-app](./todo-app/README.md) — API TODO con SQLite y `Repo<Task>`.

Desde la raíz del workspace, instala el CLI y ejecuta la migración antes de la
app:

```powershell
cargo install --path crates/rdd-cli
cd example/todo-app
rdd migrate
cargo run
```
