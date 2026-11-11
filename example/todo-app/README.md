# todo-app — demo autenticado (§26)

API TODO con **JWT**, **roles**, **CORS** y **rate limit** sobre **SQLite**. Es el
demo de aprendizaje del spec: muestra cómo se monta autenticación *a nivel de
aplicación* sin que el framework gane un crate `rdd-auth` (§2.2).

Lo único que lo separa de un `rdd new --database sqlite` recién generado es el
código de `users`, `src/middleware/`, `src/config/jwt_config.rs` y `.env`. La
migración inicial la puso el CLI (RF-192); `0002_users.sql` la añadió el demo.

## Arrancar

```bash
cp .env.example .env
# Editar .env: JWT_SECRET es obligatorio y no tiene valor por defecto.
# Generar uno: openssl rand -hex 32
rdd migrate          # crea tasks y users; el servidor no migra al arrancar (RF-117)
cargo run
```

Sin `JWT_SECRET` la app no arranca: es a propósito, para que no quede un token
firmado con una clave pública (RF-063, RN-030).

| Variable | Para qué |
|---|---|
| `JWT_SECRET` | Clave HS256. Obligatoria, solo del entorno. |
| `JWT_TTL_SECS` | Vida del token. Ausente o inválida → 3600. |
| `ADMIN_EMAIL` | El correo que se registre con este valor nace `admin`. Vacío → nadie es admin. |
| `DATABASE_URL` | `sqlite://todo-app.db`. |

## Rutas

| Método | Ruta | Quién | Respuesta |
|---|---|---|---|
| `GET` | `/` y `/health` | cualquiera | 200 |
| `POST` | `/auth/register` | cualquiera | 201 `{id, email, role}` |
| `POST` | `/auth/login` | cualquiera | 200 `{access_token, token_type, user}` |
| `GET` | `/users/me` | con sesión | 200 |
| `GET` | `/tasks?q=&done=` | con sesión | 200, solo las propias |
| `GET` | `/tasks/:id` | dueño | 200 / 404 |
| `POST` | `/tasks` | con sesión | 201 |
| `PUT` | `/tasks/:id` | dueño | 200 / 404 |
| `DELETE` | `/tasks/:id` | `admin` **y** dueño | 204 / 403 / 404 |

`RegisterDto` no lleva campo `role`: dejar que el cliente lo elija sería una
escalada de privilegios. El único camino al rol `admin` es `ADMIN_EMAIL`.

## Probarlo

```bash
curl -i -X POST localhost:3000/auth/register \
  -H 'content-type: application/json' \
  -d '{"email":"admin@example.com","password":"una-clave-larga"}'

TOKEN=$(curl -s -X POST localhost:3000/auth/login \
  -H 'content-type: application/json' \
  -d '{"email":"admin@example.com","password":"una-clave-larga"}' \
  | sed -n 's/.*"access_token":"\([^"]*\)".*/\1/p')

curl -s localhost:3000/tasks -H "authorization: Bearer $TOKEN"
curl -i -X POST localhost:3000/tasks -H "authorization: Bearer $TOKEN" \
  -H 'content-type: application/json' -d '{"title":"comprar pan"}'

# Sin token: 401. El preflight sí pasa: CORS responde antes que JWT (C-09).
curl -i localhost:3000/tasks
curl -i -X OPTIONS localhost:3000/tasks -H 'Origin: http://localhost:5173' \
  -H 'Access-Control-Request-Method: DELETE'
```

## Por qué el middleware está partido en tres

El orden de RDD es global → módulo → controller → ruta (RF-080), y eso fuerza el
desdoble:

- **`JwtMiddleware` (global) identifica, no rechaza.** Si rechazara, dejaría fuera
  `/auth/login` y `/health`. Un token inválido se registra en el log y la petición
  sigue sin sesión.
- **`RequireAuth` (controller) exige sesión** y responde 401. Va después del
  `RateLimit` global, que ya tiene identidad para su clave (RF-109).
- **`RequireRole` (ruta) exige rol** y responde 403. Solo `DELETE /tasks/:id`.

Cadena completa: `CORS → JWT → RateLimit → módulo → RequireAuth → RequireRole → handler`.

**Rol y dueño son dos autorizaciones distintas.** `RequireRole` decide si la
identidad puede ejecutar la operación; el caso de uso decide si el recurso es suyo.
El acceso cruzado responde **404, no 403**, para no revelar que la tarea existe:
un admin tampoco borra tareas ajenas.

## Puertos y adaptadores

`users/domain/` define `UserRepository`, `PasswordHasher` y `TokenCodec`;
`users/infrastructure/` los implementa con `Repo<User>`, `argon2` y
`jsonwebtoken`. Esas dos dependencias son del demo, no del núcleo (RA-014).

`TokenCodec` es un puerto a propósito: el `Claims` del JWT se escribe una sola
vez. Si emisión y verificación tuvieran cada una su copia, cambiar un campo
rompería el login en silencio.

`src/middleware/` importa `Session`, `Role` y `TokenCodec` desde `users/mod.rs`:
es la API pública que RA-004 permite, y al estar fuera de `modules/` no le
aplican RA-001 a RA-003 ni RA-007.

## Verificación

```bash
cargo test -p todo-app                          # unidad + aceptación (§26.6)
cargo clippy -p todo-app --all-targets -- -D warnings
cargo fmt -p todo-app -- --check
```

`tests/acceptance.rs` cubre las comprobaciones 2–6 y 8 de §26.6. La 1 (`rdd
migrate` + arranque) y la 7 (persistencia tras reiniciar) se hacen con el CLI y un
archivo SQLite real: parar la app, volver a arrancar y comprobar que el usuario y
las tareas siguen ahí.

Este proyecto es miembro del workspace (RF-193), así que `cargo test --workspace`
también lo compila: un ejemplo fuera del workspace se pudre sin que nada lo
detecte.