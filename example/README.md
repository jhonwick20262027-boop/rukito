# Ejemplos

Cada aplicación de ejemplo vive en su propia carpeta:

- [todo-app](./todo-app/README.md) — demo de §26: API TODO con SQLite, JWT, roles,
  CORS y rate limit. La autenticación vive en la aplicación, no en el framework.

Desde la raíz del workspace, instala el CLI, copia el entorno y migra antes de
arrancar la app:

```powershell
cargo install --path crates/rdd-cli
cd example/todo-app
Copy-Item .env.example .env   # editar JWT_SECRET: es obligatorio
rdd migrate
cargo run
```

Los ejemplos son miembros del workspace (RF-193): `cargo test --workspace`,
`cargo clippy --all-targets -- -D warnings` y `cargo fmt --all -- --check` también
los cubren.
