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
| `GRAPHQL_PATH` | Ruta del montaje con `--graphql`. Por defecto `/graphql`. |
| `GRAPHQL_EXPLORER` | `true` sirve la UI; `false` desactiva el explorer (404 tras autorizar). |
| `GRAPHQL_PUBLIC_EXPLORER` | `true` sirve la UI sin sesión; las operaciones siguen exigiendo el Bearer. |
| `GRAPHQL_MAX_DEPTH` | Anidamiento máximo de conjuntos de selección. Por defecto 16. |
| `OPENAPI_PATH` | Ruta de la UI con `--openapi`. Por defecto `/api-docs`. |
| `OPENAPI_DOCS` | `true` sirve la UI y el JSON; `false` responde 404 (tras autorizar). |
| `OPENAPI_PUBLIC_DOCS` | `true` sirve UI y JSON sin JWT (demo); en producción usar `false`. |
| `OPENAPI_ALIASES` | Alias de la UI separados por comas. Por defecto `/docs,/swagger`. |
| `OPENAPI_STRICT` | `true` aborta el arranque si una ruta montada no está documentada. |

## Modos de arranque

El REST arranca siempre; `--mcp` es excluyente porque no abre puerto, y
`--mcp-http`, `--graphql` y `--openapi` se **acumulan** sobre el mismo
`AxumPlatform`; `--explorer-token` se puede combinar con cualquiera: imprime el
Bearer y sigue arrancando.

```bash
cargo run -- --openapi                    # REST + /api-docs, /docs y /swagger
cargo run -- --graphql --openapi          # REST + explorer + documentación
cargo run -- --graphql --explorer-token   # REST + /graphql + token de preview
cargo run -- --mcp-http                   # REST + /mcp
cargo run -- --mcp                        # MCP por stdio, sin puerto
```

| Argumento | Efecto |
|---|---|
| `--openapi` | Monta la UI OpenAPI en `OPENAPI_PATH` (por defecto `/api-docs`) con alias `/docs` y `/swagger`, y el JSON en `/api-docs/openapi.json`. |
| `--graphql` | Monta `GRAPHQL_PATH` (por defecto `/graphql`) con el esquema de los casos de uso y SSE para suscripciones. |
| `--explorer-token` | Imprime `EXPLORER_TOKEN=<jwt>` para la identidad de `config/explorer_config.rs` y continúa. |
| `--mcp-http` | Monta `/mcp` sobre el mismo servidor; exige el JWT de la cadena global. |
| `--mcp` | Sirve MCP por stdio; no abre puerto. |

Con `--graphql`, `GET /graphql` sin `query` sirve la UI (sin sesión si
`GRAPHQL_PUBLIC_EXPLORER=true`) y las operaciones exigen `Authorization: Bearer`.
`--mcp-http` añade `POST` y `DELETE` en `/mcp`.

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

## Probar OpenAPI

`--openapi` sirve la documentación en `/api-docs` (alias `/docs` y `/swagger`) y
el documento en `/api-docs/openapi.json`. En el demo `OPENAPI_PUBLIC_DOCS=true`,
así que UI y JSON se leen sin JWT; las operaciones que lance *Try it* sí exigen
el Bearer.

```bash
# 1. Arranca (SQLite ya migrado, JWT_SECRET en .env)
cargo run -- --openapi

# 2. UI y JSON
curl -i localhost:3000/api-docs | head -n 15
curl -s localhost:3000/api-docs/openapi.json | jq '.info.title, (.paths | keys)'

# 3. Solo GET: el resto responde 405 con Allow: GET
curl -i -X POST localhost:3000/api-docs

# 4. Un Bearer para Try it
curl -s -X POST localhost:3000/auth/register \
  -H 'content-type: application/json' \
  -d '{"email":"ana@correo.com","password":"una-clave-larga"}'

TOKEN=$(curl -s -X POST localhost:3000/auth/login \
  -H 'content-type: application/json' \
  -d '{"email":"ana@correo.com","password":"una-clave-larga"}' \
  | sed -n 's/.*"access_token":"\([^"]*\)".*/\1/p')

curl -s localhost:3000/tasks -H "authorization: Bearer $TOKEN"
```

En el navegador, abre `http://localhost:3000/api-docs`, elige una operación y en
*Try it* pega `Bearer <TOKEN>` en la cabecera `Authorization`. El JSON que
consume la UI es el mismo que publica `/api-docs/openapi.json`, y el arranque
falla si una ruta montada no está documentada (`OPENAPI_STRICT=false` solo
avisa).

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