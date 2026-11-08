# RDD — Especificación técnica del framework REST DDD en Rust

| Campo | Valor |
|---|---|
| Documento | `RDD-SPEC.md` — **única fuente de verdad** del proyecto |
| Versión del documento | 0.4.6-draft |
| Fecha | 2026-11-07 |
| Estado | Borrador para aprobación |
| Nombre del framework | `rdd` (provisional; verificar disponibilidad de nombres de crates antes de publicar) |

---

## 0. Cómo usar este documento

1. **Este documento manda.** Si el código y el documento discrepan, se corrige el código, o se cambia primero el documento mediante un cambio explícito (ver §24).
2. **Palabras normativas:** **DEBE** = obligatorio. **NO DEBE** = prohibido. **DEBERÍA** = recomendado, con excepción justificada. **PUEDE** = opcional.
3. **Cada requisito tiene un ID** (`RF-xxx` funcional, `RN-xxx` no funcional, `RA-xxx` regla de arquitectura, `RP-xxx` requisito de plataforma HTTP). Los commits y PRs referencian los IDs.
4. **No se fijan versiones de dependencias aquí.** Al iniciar cada fase se usan las últimas versiones estables compatibles y se fijan en `Cargo.lock`. Las dependencias cuyo comportamiento cambia entre versiones mayores (Axum, Prisma CLI) están marcadas con ⚠️ en la lista de verificaciones (§22).
5. **MSRV:** Rust 1.94 (edición 2024), requerido por SQLx 0.9.

---

## 1. Visión y principios

**Visión.** Un framework REST para Rust con la ergonomía de NestJS y el minimalismo de las Minimal APIs de .NET, que guía al desarrollador hacia una organización DDD sin obligarlo a usar patrones avanzados.

**Principios de diseño**

| # | Principio | Consecuencia |
|---|---|---|
| P1 | **Simple primero** | Un crate, un binario, sin buses ni microservicios en el alcance actual. |
| P2 | **El dominio no conoce HTTP ni SQL** | `domain.rs` no importa `axum`, `sqlx` ni nada de `rdd::http`. |
| P3 | **Núcleo mínimo, extras como paquetes** | CORS, logging, Prisma son crates separados que se añaden con `cargo add`. |
| P4 | **El núcleo no depende de ningún servidor HTTP** | Axum es una *plataforma* intercambiable detrás de la interfaz `Platform` (§4.4), igual que `platform-express`/`platform-fastify` en NestJS. |
| P5 | **Errores claros** | Las macros producen diagnósticos accionables con código (§8.7). |
| P6 | **Magia acotada** | Las macros generan código predecible y documentado (§8); no hay reflexión ni comportamiento oculto en tiempo de ejecución. |
| P7 | **Honestidad técnica** | Lo que Rust no permite (reflexión, DI resuelta por tipos en tiempo de ejecución) se documenta, no se simula. |

---

## 2. Alcance

### 2.1 Dentro del alcance (fases 0.1 a 0.4)

- Servidor REST con controladores declarativos por macros, sobre una interfaz de plataforma (§4.4). **Axum es la plataforma por defecto** (`rdd-axum`).
- Building blocks DDD mínimos: `Aggregate`, `Service`, `Repo<T>`, `Error` de dominio.
- Inyección de dependencias (singleton) por contenedor.
- Sistema de middlewares propio del núcleo: globales, por módulo, por controlador y por ruta. Las capas Tower se admiten como opción de la plataforma Axum (RF-089).
- Suite de conformidad de plataformas (§4.5) y, en la fase 0.5, una segunda plataforma (`rdd-hyper`) que valide la abstracción.
- Crates externos `rdd-cors` y `rdd-logger`.
- Persistencia multi-motor **vía adaptadores**: el núcleo define el puerto `DbAdapter` y cada motor vive en su propio crate (§12). SQLite se incluye como adaptador predeterminado para proyectos locales; PostgreSQL cuenta con adaptador y MySQL, MongoDB, Redis y MSSQL quedan para fases posteriores.
- Migraciones y esquema con **Prisma CLI** y generación de código Rust a partir de `schema.prisma`.
- CLI `rdd` para scaffolding y ejecución.

### 2.2 Fuera del alcance (explícito)

| Fuera de alcance | Nota |
|---|---|
| Microservicios, bus de eventos, NATS/Kafka, clientes remotos | Eliminado a petición; el diseño no lo impide en el futuro. |
| Event Sourcing, CQRS formal, sagas | Reservado, no implementar en 0.x. |
| **Cliente Prisma para Rust** | El proyecto `prisma-client-rust` está sin mantenimiento; se usa Prisma **solo** para esquema y migraciones (§13). |
| GraphQL, WebSockets, gRPC | No. |
| Streaming de cuerpos de petición/respuesta | Cuerpos en memoria con límite en 0.x (ADR-12). |
| Capas Tower por ruta, o dentro de `App` | Solo como opción de `AxumPlatform` (RF-089). |
| Runtimes distintos de Tokio | El núcleo y las plataformas asumen Tokio (ADR-11). |
| OpenAPI automático | Candidato post-0.4. |
| Autenticación completa | Solo el mecanismo de middlewares; un crate `rdd-auth` es candidato futuro. |
| Motores sin adaptador publicado | El puerto `DbAdapter` (§12) es la vía de extensión. SQLite está implementado en `rdd-sqlite` y es el adaptador predeterminado; Oracle, Cassandra y DynamoDB no están previstos. |
| Migraciones multi-motor | Prisma (§13) sigue siendo **solo PostgreSQL**. Los demás motores usan su herramienta nativa o esquema manual; unificar migraciones es fase aparte. |
| Transacciones declarativas (`#[transactional]`) | Candidato post-0.4. |

---

## 3. Glosario DDD aplicado

| Concepto | Definición en `rdd` | Dónde vive |
|---|---|---|
| **Aggregate** | Entidad raíz que protege invariantes. Se modifica solo mediante sus métodos. | `domain.rs` |
| **Value Object** | Tipo inmutable validado al construirse (ej. `Title`). Sin identidad. | `domain.rs` |
| **Service** | Caso de uso (application service): carga, ejecuta reglas del aggregate, guarda. No contiene reglas de negocio. | `service.rs` |
| **Repository** | Abstracción de persistencia de aggregates. En `rdd` es `Repo<T>`. | Framework / `repository.rs` |
| **Controller** | Adaptador HTTP: traduce petición a llamada de servicio y resultado a respuesta. No decide nada. | `routes.rs` |
| **Module** | Carpeta que agrupa un contexto (`tasks/`) y se registra con `.module(...)`. | `<modulo>/mod.rs` |
| **Middleware** | Lógica transversal (CORS, logging, auth) que envuelve el procesamiento de la petición. | Crates / `middleware.rs` |
| **Platform** | Adaptador que conecta el `HttpApp` del núcleo con un servidor HTTP concreto (Axum, hyper). No contiene lógica de aplicación. | Crates `rdd-axum`, `rdd-hyper` |

**Regla de oro (RA-001):** las reglas de negocio viven en el aggregate. El controller nunca decide.

---

## 4. Arquitectura general

### 4.1 Workspace del framework

```
rdd/                              # repositorio del framework
├── Cargo.toml                    # [workspace]
├── RDD-SPEC.md                   # este documento
├── crates/
│   ├── rdd/                      # FACHADA: re-exporta rdd-core + rdd-macros + plataformas por feature
│   ├── rdd-core/                 # núcleo: App, DI, middlewares, errores, Repo<T>, puerto DbAdapter, HttpApp, trait Platform
│   ├── rdd-macros/               # proc-macros
│   ├── rdd-axum/                 # plataforma por defecto
│   ├── rdd-hyper/                # plataforma propia (fase 0.5)
│   ├── rdd-platform-tests/       # suite de conformidad de plataformas (§4.5)
│   ├── rdd-db-tests/             # suite de conformidad de adaptadores (§12.4)
│   ├── rdd-sqlite/               # adaptador SQLite predeterminado (sqlx)
│   ├── rdd-postgres/             # adaptador PostgreSQL (sqlx)
│   ├── rdd-mysql/                # adaptador MySQL/MariaDB (sqlx)
│   ├── rdd-mongodb/              # adaptador MongoDB (fase 0.6)
│   ├── rdd-redis/                # adaptador Redis: KV + caché (fase 0.6)
│   ├── rdd-mssql/                # adaptador SQL Server (tiberius-ng, fase 0.6)
│   ├── rdd-cors/                 # middleware CORS
│   ├── rdd-logger/               # middleware de logging / request-id
│   ├── rdd-prisma/               # parser de schema.prisma + generador
│   └── rdd-cli/                  # binario `rdd`
└── todo-app-ddd/                 # app de ejemplo (§25): la genera `rdd new`, miembro del workspace
```

**Por qué hay un crate `rdd-core` y otro `rdd`.** Si el núcleo se llamara `rdd` y además incluyera `rdd-axum` como feature, habría una dependencia circular (`rdd-axum` necesita el núcleo). Por eso el núcleo es `rdd-core` y `rdd` es una **fachada** que lo re-exporta junto con las macros y activa la plataforma por feature. El usuario solo declara `rdd` (y los paquetes opcionales como `rdd-cors`).

```toml
# Cargo.toml del usuario
rdd      = "..."                          # axum + sqlite por defecto (§12.6)
rdd-cors = "..."
uuid     = { version = "1", features = ["v4", "serde"] }   # el id del agregado (§7.2)
# o, sin Axum:
# rdd = { version = "...", default-features = false, features = ["hyper", "sqlite"] }
```

### 4.2 Dependencias entre crates (RA-010)

```
rdd-macros ──┐
rdd-core ◄───┼── rdd (fachada) ──► [feature axum]  rdd-axum
     ▲       │                 └─► [feature hyper] rdd-hyper
     ├── rdd-axum, rdd-hyper
     ├── rdd-cors, rdd-logger
     ├── rdd-sqlite, rdd-postgres, rdd-mysql, rdd-mongodb, rdd-redis, rdd-mssql
     ├── rdd-platform-tests ──► (dev) cada plataforma
     └── rdd-db-tests ────────► (dev) cada adaptador
rdd-prisma ──► rdd-cli
```

| Crate | Puede depender de | NO DEBE depender de |
|---|---|---|
| `rdd-core` | `http`, `bytes`, `futures`, `tokio`, `serde`, `serde_json`, `tracing`, `uuid`, `thiserror`, `anyhow` | `axum`, `hyper`, `tower-http`, cualquier plataforma, **cualquier driver de BD** (`sqlx`, `mongodb`, `redis`, `tiberius`), Prisma, Node |
| `rdd-macros` | `syn`, `quote`, `proc-macro2`, crate de diagnósticos | `rdd-core` |
| `rdd` | `rdd-core`, `rdd-macros`, plataformas y adaptadores (opcionales por feature) | — |
| `rdd-sqlite` | `rdd-core`, `sqlx` (feature SQLite), `tokio`, `anyhow` | cualquier otro adaptador, cualquier plataforma |
| `rdd-axum` | `rdd-core`, `axum`, `tower`, `tower-http` | `rdd-hyper` |
| `rdd-hyper` | `rdd-core`, `hyper`, `hyper-util` | `rdd-axum`, `axum` |
| `rdd-cors`, `rdd-logger` | `rdd-core` | `axum`, `hyper`, `tower`, `tower-http`, cualquier plataforma |
| `rdd-postgres`, `rdd-mysql` | `rdd-core`, `sqlx` (feature del motor), `tokio`, `anyhow` | cualquier otro adaptador, cualquier plataforma |
| `rdd-mongodb` | `rdd-core`, `mongodb`, `tokio`, `anyhow` | `sqlx`, cualquier otro adaptador |
| `rdd-redis` | `rdd-core`, cliente Redis, `tokio`, `anyhow` | `sqlx`, cualquier otro adaptador |
| `rdd-mssql` | `rdd-core`, `tiberius-ng`, `bb8`, `tokio-util`, `anyhow` | `sqlx`, cualquier otro adaptador |
| `rdd-platform-tests` | `rdd-core`; las plataformas como `dev-dependencies` | — |
| `rdd-db-tests` | `rdd-core`, `uuid`, `serde_json`; `tokio` como `dev-dependency` | cualquier adaptador, cualquier driver |
| `rdd-prisma` | `serde`, `thiserror` | `axum`, `sqlx`, `rdd-core` |
| `rdd-cli` | `rdd-prisma`, `clap`, `sqlx` (Any, migraciones SQLite/PostgreSQL), `tokio`, `toml`, `dotenvy` | `rdd-core` (el CLI no enlaza el framework) |

`anyhow` entra en los adaptadores porque `Error::internal` recibe un `anyhow::Error`
(RF-042): así el error del driver se conserva como fuente en vez de aplanarse a
texto. La suite de conformidad va en la dirección contraria a la que sugería su
nombre: **cada adaptador depende de `rdd-db-tests` como `dev-dependency`**, nunca al
revés, para que la suite no arrastre ningún driver.

| ID | Requisito |
|---|---|
| RA-011 | `rdd-core`, `rdd-cors` y `rdd-logger` NO DEBEN tener a `axum` ni `hyper` en su árbol de dependencias. CI DEBE verificarlo (`cargo tree -p rdd-core` y regla `bans` de `cargo-deny`). |
| RA-012 | Las plataformas NO DEBEN depender entre sí. |
| RA-013 | Las macros generan rutas absolutas `::rdd::...` (RN-012); por tanto todo proyecto de usuario depende de `rdd` (la fachada), nunca solo de `rdd-core`. |
| RA-014 | `rdd-core` NO DEBE tener ningún driver de base de datos en su árbol de dependencias. El puerto `DbAdapter` (§12.1) es lo único que el núcleo conoce. CI DEBE verificarlo igual que RA-011. |
| RA-015 | Los adaptadores NO DEBEN depender entre sí, ni de plataforma alguna. Cada uno depende de `rdd-core` y de su driver. |
| RA-016 | Todo adaptador publicado DEBE pasar `rdd-db-tests` (§12.4) contra un servidor real de su motor. Un adaptador sin servidor donde verificarse NO se publica: queda como verificación pendiente en §22. |

### 4.3 Stack tecnológico

| Necesidad | Elección | Dónde | Razón |
|---|---|---|---|
| Runtime async | `tokio` | núcleo | Estándar de facto (ADR-11). |
| Tipos HTTP | crate `http` (`Method`, `StatusCode`, `HeaderMap`, `Request`, `Response`) + `bytes` | núcleo | Neutrales: los comparten Axum, hyper y Tower. |
| Plataforma por defecto | `axum` (⚠️ V1) | `rdd-axum` | Router y servidor maduros. |
| Plataforma propia | `hyper` + `hyper-util` (⚠️ V9) | `rdd-hyper` | Control total, valida la abstracción. |
| Capas Tower | `tower`, `tower-http` | solo `rdd-axum` | Opcionales vía `AxumPlatform::layer` (RF-089). |
| Puerto de persistencia | `DbAdapter` propio (§12.1) | núcleo | Sin drivers: el núcleo no conoce ningún motor (RA-014). |
| SQLite / PostgreSQL / MySQL | `sqlx` 0.9 | `rdd-sqlite`, `rdd-postgres`, `rdd-mysql` | Async, sin ORM pesado; SQLite es el adaptador predeterminado para proyectos locales. |
| MongoDB | `mongodb` 3.x (oficial) | `rdd-mongodb` | Driver oficial async. Documental: no hay SQL ni `JOIN`. |
| Redis | cliente async (⚠️ V13) | `rdd-redis` | KV: agregados simples por clave + caché/sesiones. |
| SQL Server | `tiberius-ng` 0.13 + `bb8` (⚠️ V12) | `rdd-mssql` | `sqlx` eliminó MSSQL en 0.7 y `tiberius` original está sin mantenimiento. |
| Serialización | `serde`, `serde_json` | núcleo | Estándar. |
| Errores | `thiserror` + `anyhow` (interno) | núcleo | Errores tipados en el borde, contexto interno. |
| Trazas | `tracing`, `tracing-subscriber` | núcleo | Estándar. |
| IDs | `uuid` | núcleo | Estándar. |
| Macros | `syn`, `quote`, `proc-macro2`, crate de diagnósticos (⚠️ V6) | `rdd-macros` | Diagnósticos claros. |
| CLI y migraciones | `clap`, `sqlx` (Any, SQLite/PostgreSQL), `tokio`, `toml` | `rdd-cli` | `rdd migrate` ejecuta SQL versionado antes del despliegue. |
| Esquema/migraciones | `sqlx` 0.9 para migraciones versionadas runtime; Prisma CLI para schema y generación | `rdd-cli` / herramienta externa | `rdd migrate` aplica SQL versionado; Prisma continúa según §13. |

### 4.4 Interfaz de plataforma (modelo "adaptador sobre el router del servidor")

El núcleo define **qué** es una aplicación HTTP (rutas como datos, con middlewares ya compuestos). La plataforma solo sabe **servirla**.

```rust
// rdd-core
pub type Request   = http::Request<bytes::Bytes>;     // cuerpo ya en memoria (RP-002)
pub type Response  = http::Response<bytes::Bytes>;
pub type BoxHandler = Arc<dyn Fn(Request) -> BoxFuture<'static, Response> + Send + Sync>;

pub struct RouteDef {
    pub method:  Method,
    pub path:    String,              // SIEMPRE sintaxis "/:id" (RP-003); es `String` porque la
                                      // ruta final es `prefijo + ruta` y se calcula al montar (RF-051)
    pub handler: BoxHandler,          // incluye su cadena completa de middlewares
}

pub struct Limits {
    pub body_limit_bytes: usize,      // RF-017
    pub shutdown_grace:   Duration,   // RP-009
}

pub struct HttpApp {
    pub routes:   Vec<RouteDef>,
    pub fallback: BoxHandler,         // decide 404 o 405 (RP-007); también envuelto por middlewares globales
    pub limits:   Limits,
}
impl HttpApp {
    /// Enrutado y ejecución sin servidor: usado por TestApp y por plataformas sin router propio (RP-014).
    pub async fn dispatch(&self, req: Request) -> Response;
}

pub trait Platform: Send + 'static {
    fn name(&self) -> &'static str;
    fn serve(self, app: HttpApp, listener: tokio::net::TcpListener, shutdown: ShutdownSignal)
        -> BoxFuture<'static, Result<()>>;
}
```

`App::run(addr)` usa la plataforma por defecto; `App::run_on(plataforma, addr)` usa la indicada:

```rust
App::new().module(tasks::module()).run("0.0.0.0:3000").await;                          // Axum (feature por defecto)
App::new().module(tasks::module()).run_on(HyperPlatform::new(), "0.0.0.0:3000").await; // plataforma propia
```

**Reparto de responsabilidades**

| Lo hace el núcleo (idéntico en toda plataforma) | Lo hace la plataforma |
|---|---|
| Composición y orden de los middlewares | Registrar/ejecutar las rutas con el router del servidor |
| Extractores (`Json`, `Path`, `Query`, servicios) | Traducir `/:id` a la sintaxis de su router y rellenar `PathParams` |
| DI y conversión de `Error` a respuesta (Problem Details) | Aceptar conexiones y leer cuerpos con límite |
| Respuestas 404/405/413 (cuerpos y formato) | Normalizar la barra final antes de enrutar |
| Captura de `panic` por handler | Drenar peticiones en curso al apagar |
| Abrir el `TcpListener` y capturar señales (`SIGTERM`, `Ctrl+C`) | Servir sobre el `TcpListener` recibido |

**Requisitos de plataforma**

| ID | Requisito |
|---|---|
| RP-001 | El núcleo NO DEBE conocer ninguna plataforma concreta (RA-011). |
| RP-002 | `Request` y `Response` son `http::Request<Bytes>` / `http::Response<Bytes>`. El cuerpo se lee completo en memoria, respetando el límite (RP-006). El streaming queda fuera de 0.x (ADR-12). |
| RP-003 | Las rutas se declaran con `/:nombre`. La plataforma DEBE traducir esa sintaxis a la de su router y DEBE insertar los parámetros capturados en `req.extensions()` como `PathParams`. La traducción es un detalle interno de la plataforma (esto resuelve V1 sin afectar al usuario). |
| RP-004 | Los handlers son **infalibles** a este nivel: el núcleo ya convirtió todo `Error` en `Response`. La plataforma NO DEBE inspeccionar ni reescribir respuestas de handlers. |
| RP-005 | La plataforma NO DEBE reordenar ni insertar middlewares *entre* los de `rdd`. Solo puede envolver el conjunto completo por fuera (RF-089). |
| RP-006 | Si el cuerpo supera `limits.body_limit_bytes`, la plataforma DEBE responder `413` en formato Problem Details (`HttpApp::payload_too_large()`) sin leer el resto del cuerpo, antes de invocar al handler. |
| RP-007 | Si ninguna ruta coincide, la plataforma DEBE invocar `fallback`. Si la ruta existe con otros métodos, DEBE añadir `AllowedMethods(Vec<Method>)` a las extensiones; el `fallback` decide entre 404 y 405 (con cabecera `Allow`). |
| RP-008 | La plataforma DEBE tratar `/tasks/` y `/tasks` como la misma ruta (RF-018). |
| RP-009 | Al completarse `ShutdownSignal`, la plataforma DEBE dejar de aceptar conexiones nuevas y esperar a las peticiones en curso hasta `shutdown_grace` (por defecto 30 s); pasado ese plazo, DEBE cerrar. |
| RP-010 | Un fallo de una conexión individual NO DEBE detener el servidor. |
| RP-011 | Las capacidades específicas de un servidor (p. ej. capas Tower) DEBEN exponerse como opciones del constructor de la plataforma, nunca en `App`. |
| RP-012 | `run()` solo existe si la feature `axum` está activa; con otra configuración, el usuario DEBE usar `run_on` (error de compilación claro si no). |
| RP-013 | Toda plataforma DEBE implementar `name()`; el núcleo lo registra al arrancar (RN-042). |
| RP-014 | `HttpApp::dispatch` DEBE implementarse en el núcleo con una tabla de rutas propia (⚠️ V8). Es la **plataforma de referencia** en proceso: `TestApp` la usa (RF-160) y `rdd-hyper` puede apoyarse en ella. |

**Nota sobre `rdd-axum`:** registra las rutas de forma **nativa** en el router de Axum (no usa `dispatch`), para aprovechar su enrutado y el soporte de capas Tower. Como hay dos implementaciones de enrutado (Axum y `dispatch`), el riesgo de divergencia se controla con la suite de conformidad (§4.5), que se ejecuta contra ambas.

### 4.5 Suite de conformidad (`rdd-platform-tests`)

Una abstracción con un solo adaptador tiende a copiar la forma de ese adaptador. La suite impide eso: **toda plataforma debe pasar los mismos casos** contra el mismo `HttpApp`.

```rust
// en el crate de cada plataforma, como test de integración:
#[tokio::test]
async fn conformance() {
    rdd_platform_tests::run_all(|| AxumPlatform::new()).await;
}
```

La suite levanta la plataforma en un puerto efímero (`127.0.0.1:0`, por eso el núcleo abre el `TcpListener`, §4.4) y ejecuta peticiones HTTP reales.

| Caso | Verifica | Requisitos |
|---|---|---|
| C-01 | Orden de ejecución de tres middlewares anidados (global → módulo → controller → ruta) | RF-080 |
| C-02 | 404 en Problem Details; 405 con cabecera `Allow` | RF-072, RP-007 |
| C-03 | `/tasks` y `/tasks/` responden igual | RF-018, RP-008 |
| C-04 | Parámetros de ruta (`/tasks/:id/complete` con UUID) y UUID inválido → 400 | RF-055, RP-003 |
| C-05 | Cuerpo mayor al límite → 413 sin invocar al handler | RF-017, RP-006 |
| C-06 | JSON inválido → 400 | RF-055 |
| C-07 | `panic` en un handler → 500 y el servidor sigue vivo | RF-073, RP-010 |
| C-08 | Middleware global cubre respuestas 404/405 | RF-081 |
| C-09 | Preflight CORS responde sin ejecutar middlewares internos | RF-094, RF-096 |
| C-10 | `x-request-id` generado y propagado | RF-101 |
| C-11 | Apagado ordenado: una petición lenta en curso termina; una nueva es rechazada | RF-013, RP-009 |
| C-12 | 200 peticiones concurrentes responden correctamente | RP-010 |

| ID | Requisito |
|---|---|
| RP-020 | Una plataforma NO DEBE publicarse si no pasa el 100 % de la suite. |
| RP-021 | La suite DEBE ejecutarse también contra `dispatch` (plataforma de referencia, sin red) para detectar divergencias del núcleo. |
| RP-022 | Todo caso nuevo de comportamiento HTTP DEBE añadirse a la suite en la misma PR que cambia el requisito. |

### 4.6 Costos asumidos

| Costo | Mitigación |
|---|---|
| Mínimo común denominador: lo que un servidor hace y otro no queda fuera de la interfaz | Extensiones como opciones de la plataforma (RP-011) |
| Una conversión extra de tipos por petición | Tipos `http` compartidos; medir (⚠️ V10) |
| Cuerpos en memoria | Límite por defecto de 2 MB (RF-017); streaming diferido |
| Dos implementaciones de enrutado (Axum y `dispatch`) | Suite de conformidad sobre ambas (RP-021) |
| Más código a mantener (`rdd-cors` propio, extractores propios, suite) | Alcance acotado por fases (§21) |

---

## 5. Estructura de un proyecto de usuario

La estructura sigue la **convención NestJS**: un módulo por dominio de negocio y
una carpeta por responsabilidad. La adaptación a Rust es mecánica y se documenta
aquí para que el scaffold sea determinista:

- `x.y.ts` → `x_y.rs`: el punto no es válido en un identificador de módulo Rust.
- `__tests__/` → `tests/`: evita el aviso `non_snake_case` de rustc.
- `external-services/`, `payment-gateways/` → `external_services/`, `payment_gateways/`.
- `src/test/` y `test/` → `tests/` en la raíz: es el único directorio de tests de
  integración que cargo reconoce.
- `.prettierrc` → `rustfmt.toml`; `.eslintrc.json` → `clippy.toml`;
  `tsconfig.json` → `Cargo.toml` (`edition` + `rust-version`) y
  `rust-toolchain.toml` (perfil y componentes).
- `rust-toolchain.toml` NO fija `channel`: un canal pinned obliga a rustup a
  descargar otro toolchain y el proyecto dejaría de compilar sin red, lo que
  rompería RF-143. El mínimo soportado lo declara `rust-version` en `Cargo.toml`.
- Los nombres en singular de NestJS (`user.entity.ts` dentro de `users/`) se
  conservan: el CLI singulariza el nombre del módulo para `entities/`, `dto/`,
  `interfaces/` y `mappers/`, y lo deja en plural para `controllers/`,
  `services/`, `repositories/` y `constants/`.

```
todo-app-ddd/
├── Cargo.toml                    # ≈ package.json + tsconfig.json
├── rdd.toml                      # configuración (§15)
├── rust-toolchain.toml           # perfil y componentes; el MSRV va en Cargo.toml
├── rustfmt.toml                  # ≈ .prettierrc
├── clippy.toml                   # ≈ .eslintrc.json
├── Dockerfile
├── .env                          # NO versionado (RF-063)
├── .env.example
├── .gitignore                    # incluye .env y target/ (RF-063)
├── docs/
├── prisma/
│   ├── schema.prisma             # fuente de verdad del esquema de datos
│   └── migrations/               # generadas por Prisma, versionadas en git
├── tests/                        # integración (cargo)
│   ├── acceptance.rs             # recorre §10.3; aceptación de 0.1
│   ├── e2e/
│   └── fixtures/
└── src/
    ├── main.rs                   # ≈ main.ts
    ├── app_module.rs             # ≈ app.module.ts
    ├── app_controller.rs         # ≈ app.controller.ts
    ├── app_service.rs            # ≈ app.service.ts
    ├── config/
    │   ├── mod.rs                # ≈ configuration.ts
    │   ├── app_config.rs
    │   ├── database_config.rs
    │   ├── jwt_config.rs
    │   └── redis_config.rs
    ├── common/
    │   ├── mod.rs
    │   ├── decorators/mod.rs
    │   ├── filters/mod.rs
    │   ├── guards/mod.rs
    │   ├── interceptors/mod.rs
    │   ├── middleware/mod.rs
    │   ├── pipes/mod.rs
    │   ├── constants/mod.rs
    │   ├── enums/mod.rs
    │   ├── interfaces/mod.rs
    │   ├── exceptions/mod.rs
    │   └── utils/mod.rs
    ├── modules/
    │   ├── mod.rs                # marcador `// rdd:mods` (§14.3)
    │   └── tasks/                # módulo de ejemplo; nombre fijo (§14.1)
    │       ├── mod.rs            # ≈ tasks.module.ts
    │       ├── controllers/
    │       │   ├── mod.rs
    │       │   └── tasks_controller.rs
    │       ├── services/
    │       │   ├── mod.rs
    │       │   └── tasks_service.rs
    │       ├── repositories/
    │       │   ├── mod.rs
    │       │   └── tasks_repository.rs
    │       ├── dto/
    │       │   ├── mod.rs
    │       │   ├── create_task_dto.rs
    │       │   ├── update_task_dto.rs
    │       │   └── query_task_dto.rs
    │       ├── entities/
    │       │   ├── mod.rs
    │       │   └── task_entity.rs
    │       ├── interfaces/
    │       │   ├── mod.rs
    │       │   └── task_interface.rs
    │       ├── mappers/
    │       │   ├── mod.rs
    │       │   └── task_mapper.rs
    │       ├── constants/
    │       │   ├── mod.rs
    │       │   └── tasks_constants.rs
    │       └── tests/
    │           ├── mod.rs
    │           ├── tasks_service_test.rs
    │           └── tasks_controller_test.rs
    ├── infrastructure/
    │   ├── mod.rs
    │   ├── database/mod.rs
    │   ├── cache/mod.rs
    │   ├── queue/mod.rs
    │   ├── storage/mod.rs
    │   ├── messaging/mod.rs
    │   └── external_services/mod.rs
    ├── integrations/
    │   ├── mod.rs
    │   ├── aws/mod.rs
    │   ├── telegram/mod.rs
    │   ├── email/mod.rs
    │   └── payment_gateways/mod.rs
    ├── shared/
    │   ├── mod.rs
    │   ├── types/mod.rs
    │   ├── interfaces/mod.rs
    │   └── contracts/mod.rs
    └── generated/                # generado por `rdd db generate`; NO se edita
        └── mod.rs
```

### 5.1 Responsabilidad de cada carpeta

| Carpeta | Responsabilidad | Equivalente NestJS |
|---|---|---|
| `modules/<m>/controllers/` | Adaptador HTTP: extrae, llama al service, devuelve. No decide (§3, RA-003). | `controllers/` |
| `modules/<m>/services/` | Casos de uso y reglas de negocio. | `services/` |
| `modules/<m>/repositories/` | Acceso a datos; envuelve `Repo<T>` (§12) o consultas especiales. | `repositories/` |
| `modules/<m>/dto/` | Contratos de entrada/salida y validación. | `dto/` |
| `modules/<m>/entities/` | Agregados y value objects del dominio. | `entities/` |
| `modules/<m>/interfaces/` | Traits del dominio (p. ej. el puerto del repositorio). | `interfaces/` |
| `modules/<m>/mappers/` | Transformación entidad ↔ DTO ↔ fila. | `mappers/` |
| `modules/<m>/constants/` | Constantes del módulo. | `constants/` |
| `modules/<m>/tests/` | Tests unitarios del módulo (`#[cfg(test)]`). | `__tests__/` |
| `common/` | Componentes transversales propios del proyecto. | `common/` |
| `infrastructure/` | Servicios técnicos: pool, caché, colas, almacenamiento. | `infrastructure/` |
| `integrations/` | Clientes de sistemas externos. | `integrations/` |
| `shared/` | Tipos, interfaces y contratos compartidos entre módulos. | `shared/` |
| `config/` | Lectura de configuración por dominio, centralizada. | `config/` |

**Dependencias permitidas** (misma dirección que §3): `controllers` → `services`
→ `entities`/`interfaces`; `repositories` implementa los `interfaces` y habla con
`infrastructure`. Un módulo NO importa archivos internos de otro (RA-004).

### 5.2 Requisitos de la estructura

| ID | Requisito |
|---|---|
| RF-001 | `rdd g module <name>` DEBE generar el árbol de módulo de §5 (nueve carpetas con su `mod.rs` y los archivos nombrados arriba) y registrarlo en `src/modules/mod.rs` y `src/main.rs` (§14.3). |
| RF-002 | `rdd new <name>` DEBE generar el árbol completo de §5, incluido el módulo de ejemplo `tasks`, salvo `--no-example`. |
| RF-003 | Toda carpeta del scaffold que sea un módulo Rust DEBE llevar un `mod.rs`; las que no lo son (`docs/`, `prisma/migrations/`, `tests/e2e/`, `tests/fixtures/`) DEBEN llevar `.gitkeep`. Ninguna carpeta queda vacía. |
| RF-004 | Cada `mod.rs` generado DEBE documentar en su cabecera qué va ahí y qué API de `rdd` lo cubre. Las carpetas cuya soporte llega en una fase posterior (§21) se generan como placeholder compilable, nunca como código que no compila. |
| RF-005 | El singular de los nombres de archivo se calcula con una regla fija y documentada: `ies`→`y`; `ses`/`xes`/`zes`/`ches`/`shes`→quita `es`; `s` final (salvo `ss`)→quita `s`; en otro caso, igual. |
| RF-006 | El scaffold NO DEBE generar código que dependa de macros o APIs aún no implementadas. En 0.1 `services/` y `entities/` son Rust plano; `#[service]` y `#[aggregate]` llegan en 0.2 y el CLI los añadirá entonces. |

**RF-007** `rdd new` y `rdd g module` DEBEN producir un proyecto que pasa
`cargo check` sin edits manuales (RF-143), y `tests/acceptance.rs` DEBE pasar.
El scaffold DEBE además nacer limpio: `cargo clippy --all-targets -- -D warnings`
y `cargo fmt --all -- --check` sin diffs, porque el proyecto ya incluye
`clippy.toml` y `rustfmt.toml` (§5) y sería contradictorio entregarlos con el
código que los viola. El test de aceptación (§21) lo verifica también después de
añadir un módulo de varias palabras (`task-items`), que es donde una plantilla se
rompe por longitud de línea o por singularización.

---

## 6. Núcleo: `App`, `Module`, contenedor

### 6.1 `App`

```rust
App::new()
    .database(env("DATABASE_URL"))   // opcional; sin esto, sin Repo SQL
    .middleware(Cors::permissive())  // global (§11)
    .middleware(Logger::default())
    .module(tasks::module())
    .run("0.0.0.0:3000")
    .await
```

| ID | Requisito |
|---|---|
| RF-010 | `App::new()` DEBE crear una aplicación vacía válida (responde 404 a todo). |
| RF-011 | `.module(Module)` PUEDE llamarse múltiples veces; el orden de registro no afecta a la resolución de dependencias. |
| RF-012 | `.run(addr)` DEBE: construir el contenedor, validar dependencias, crear el pool, componer rutas y middlewares en un `HttpApp`, abrir el `TcpListener` y entregar todo a la plataforma (§4.4). |
| RF-013 | La aplicación DEBE apagarse ordenadamente ante `SIGTERM` y `Ctrl+C`, terminando las peticiones en curso. |
| RF-014 | `env("X")` DEBE resolverse en el arranque; si falta, el proceso termina con un mensaje que nombra la variable. |
| RF-015 | `.in_memory()` DEBE sustituir el backend de persistencia por uno en memoria (desarrollo y tests, §20). Tiene prioridad sobre una URL ya configurada. |
| RF-016 | La app DEBERÍA exponer `GET /health` (200 con `{"status":"ok"}`), desactivable con `.without_health()`. |
| RF-017 | Todo cuerpo de petición DEBE tener límite de tamaño por defecto de 2 MB, configurable con `.body_limit(bytes)`. |
| RF-018 | Las rutas DEBEN normalizar la barra final: `/tasks` y `/tasks/` responden igual. La implementación es responsabilidad de cada plataforma (RP-008) y la verifica la suite de conformidad (C-03). |
| RF-019 | `.run_on(plataforma, addr)` DEBE aceptar cualquier tipo que implemente `Platform` (§4.4). |

### 6.2 `Module`

```rust
pub fn module() -> Module {
    module! {
        providers:   [TaskService],
        controllers: [TaskController],
        middlewares: [],              // opcional: aplican solo a este módulo
    }
}
```

Forma abreviada permitida: `module!(TaskService, TaskController)` — la macro clasifica cada tipo por el atributo con el que fue declarado (`#[service]` → provider, `#[controller]` → controller).

### 6.3 Contenedor de dependencias (DI)

**Limitación asumida (P7):** Rust no tiene reflexión. La DI de `rdd` **no se resuelve en compilación por completo**: las macros generan constructores tipados y el contenedor valida el grafo **al arrancar**.

| ID | Requisito |
|---|---|
| RF-020 | Todo provider es **singleton**: una instancia creada al arrancar, compartida vía `Arc`. |
| RF-021 | `#[service]` DEBE generar `impl FromContainer for T` que construye el struct tomando cada campo del contenedor por tipo. |
| RF-022 | Tipos inyectables: otros services, `Repo<T>`, `Config<T>`, `Arc<T>` de recursos registrados con `.provide(valor)`. |
| RF-023 | El contenedor DEBE construir los providers en orden topológico. |
| RF-024 | Si falta una dependencia, el arranque DEBE fallar con mensaje: tipo solicitante, campo, tipo faltante y cómo registrarlo. |
| RF-025 | Una dependencia circular DEBE detectarse al arrancar y reportar el ciclo completo (`A → B → A`). |
| RF-026 | Los services DEBEN ser `Clone + Send + Sync + 'static` (campos `Arc` internamente). |
| RF-027 | `rdd check` DEBERÍA detectar dependencias faltantes sin ejecutar la app (análisis estático básico, mejor esfuerzo). |
| RF-028 | Scope por petición (request-scoped): **fuera del alcance** en 0.x. |

**Ejemplo de código generado por `#[service]`** (referencia normativa de forma, no de nombres internos):

```rust
// El usuario escribe:
#[service]
pub struct TaskService { repo: Repo<Task> }

// La macro genera (simplificado):
impl ::rdd::di::FromContainer for TaskService {
    fn from_container(c: &::rdd::di::Container) -> ::rdd::Result<Self> {
        Ok(Self { repo: c.resolve::<Repo<Task>>("TaskService", "repo")? })
    }
}
// Extractor propio del núcleo (no depende de ninguna plataforma), con la firma
// de RF-057: `Ctx` y `Bind`, no un `Request` suelto.
impl ::rdd::http::FromRequest for TaskService {
    async fn from_request(ctx: &::rdd::http::Ctx, _bind: ::rdd::http::Bind) -> ::rdd::Result<Self> {
        ctx.resolve::<Self>()
    }
}
```

**Las dos impls son por tipo, nunca blanket.** `#[controller]` convierte cada
parámetro en `::rdd::api::extract::<T>(…)`, que exige `T: FromRequest` (RF-057). Un
`impl<T: FromContainer> FromRequest for T` solaparía con las impls locales de
`Json<T>`, `Path<T>`, `Query<T>`, `Header<T>` y `Ext<T>`: Rust no razona en
negativo, así que no puede demostrar que esos tipos nunca implementarán
`FromContainer` y rechaza el blanket con E0119. Por eso `#[service]` emite la impl
del tipo concreto, y por eso el service llega al handler como `svc: TaskService`
sin que el controller nombre el contenedor.

**Mientras `#[service]` no exista (RF-006), el scaffold escribe esas dos impls a
mano** en `services/`, con la misma forma que la macro emitirá. Un service cuya
única dependencia es `Repo<T>` no necesita entrada en `providers:` del módulo:
`App::build()` registra el valor `Db` y `Repo<T>: FromContainer` lo lee (§12.1),
así que `from_container` se limita a `c.resolve::<Repo<T>>("TaskService", "repo")`.
El singleton de RF-020 es el **adaptador** —el pool vive en `Db`— y el service es
un handle sin estado que se reconstruye por petición a partir de él; registrar el
service como provider sería redundante y obligaría a declarar `Repo<T>` como
dependencia del grafo, cuando no es un valor registrado sino un tipo resoluble.

---

## 7. Building blocks DDD

### 7.1 `Aggregate`

```rust
pub trait Aggregate: Send + Sync + 'static {
    type Id: Clone + Send + Sync + 'static;
    fn id(&self) -> &Self::Id;

    /// Reservado para eventos de dominio. Implementación por defecto: vacío.
    /// NO implementar comportamiento en 0.x.
    fn take_events(&mut self) -> Vec<Box<dyn DomainEvent>> { Vec::new() }
}
```

### 7.2 `#[aggregate]`

```rust
#[aggregate(table = "tasks")]
pub struct Task {
    #[id]
    pub id: Uuid,
    pub title: String,
    pub done: bool,
}
```

| ID | Requisito |
|---|---|
| RF-030 | DEBE existir exactamente un campo `#[id]`; si no, error `RDD0101`. |
| RF-031 | La macro DEBE generar `impl Aggregate` y añadir `#[derive(Debug, Clone, Serialize)]`. NO DEBE derivar `Deserialize` (el dominio no se construye desde JSON). |
| RF-032 | Con `table = "..."` la macro DEBE generar `impl Persist` (§12) mapeando campos a columnas del mismo nombre en snake_case. |
| RF-033 | Con `row = RowType` (en vez de `table`), el aggregate DEBE implementar `From<RowType>` y `Into<RowType>`; así el dominio queda separado de la forma de la tabla (ver ADR-05). |
| RF-034 | Los campos DEBERÍAN ser privados salvo `id`; los métodos del aggregate imponen las invariantes. El modo simple permite campos `pub`. |

**Guía (no obligatoria):** devolver el aggregate directamente desde un controller es aceptable en el modo simple. En proyectos grandes DEBERÍA usarse un DTO (`impl From<&Task> for TaskDto`).

### 7.3 Value Objects

Se escriben a mano con constructor validado; el framework no impone macro en 0.x.

```rust
pub struct Title(String);
impl Title {
    pub fn new(s: impl Into<String>) -> Result<Self> {
        let s = s.into();
        if s.trim().is_empty() { return Err(Error::invalid("el título no puede estar vacío")); }
        Ok(Self(s))
    }
}
```

### 7.4 `Error` y `Result`

```rust
pub type Result<T, E = Error> = std::result::Result<T, E>;

pub enum Error {
    BadRequest(String),    // petición malformada
    Invalid(String),       // regla de dominio / validación
    Unauthorized,
    Forbidden,
    NotFound(String),
    Conflict(String),
    Internal(anyhow::Error),
}
```

| ID | Requisito |
|---|---|
| RF-040 | Constructores: `Error::invalid(msg)`, `bad_request`, `not_found`, `conflict`, `unauthorized`, `forbidden`. |
| RF-041 | `Error::Internal` DEBE registrar el detalle con `tracing::error!` y NO DEBE exponerlo al cliente. |
| RF-042 | Cada adaptador DEBE convertir el error de su driver a `Error` con el mismo mapa: fila/documento no encontrado → `NotFound`; violación de unicidad → `Conflict`; resto → `Internal`. La conversión vive en el crate del motor, no en el núcleo (RA-014): `rdd-core` no conoce `sqlx::Error` ni ningún otro error de driver. |

---

## 8. Macros

### 8.1 Catálogo

| Macro | Aplica a | Efecto |
|---|---|---|
| `#[rdd::main]` | `async fn main` | Crea el runtime Tokio, inicializa `tracing` y carga `.env`. |
| `#[aggregate(...)]` | `struct` | §7.2 |
| `#[service]` | `struct` | §6.3 |
| `#[controller("/ruta")]` | `impl` | §8.2 |
| `#[get]` `#[post]` `#[put]` `#[patch]` `#[delete]` | método dentro de `#[controller]` | §8.2 |
| `#[middleware(expr)]` | `impl` de controller o método | §11.4 |
| `module!` | expresión | §6.2 |

### 8.2 `#[controller]` y rutas

```rust
pub struct TaskController;            // DEBE declararse explícitamente

#[controller("/tasks")]
impl TaskController {
    #[get("/")]
    async fn list(svc: TaskService) -> Result<Vec<Task>> { svc.list().await }

    #[post("/")]
    async fn create(svc: TaskService, body: Json<NewTask>) -> Result<Task> {
        svc.create(body.title.clone()).await
    }

    #[put("/:id/complete")]
    async fn complete(svc: TaskService, id: Path<Uuid>) -> Result<Task> {
        svc.complete(*id).await
    }
}
```

| ID | Requisito |
|---|---|
| RF-050 | Los métodos del controller son **funciones asociadas sin `self`**; los parámetros son extractores. |
| RF-051 | La ruta final es `prefijo + ruta`, normalizando barras: `"/tasks"` + `"/"` → `/tasks`; `"/tasks"` + `"/:id"` → `/tasks/:id`. |
| RF-052 | `Result<T>` donde `T: Serialize` DEBE convertirse a respuesta JSON. |
| RF-053 | Código de estado por defecto: `POST` → **201**, `()` → **204**, resto → **200**. Se sobrescribe con `#[post("/", status = 202)]`. |
| RF-054 | Extractores disponibles: `Json<T>`, `Path<T>`, `Query<T>`, `Header<T>`, `Ext<T>` (extensiones puestas por middlewares), cualquier `#[service]`, y `Config<T>`. `Json`, `Path` y `Query` implementan `Deref` hacia su contenido. |
| RF-055 | Si un extractor falla (JSON inválido, UUID inválido), la respuesta DEBE ser `400` con el formato de §10.3. |
| RF-056 | El método `routes()` generado (`TaskController::routes()`) DEBE devolver `Vec<RouteDef>` (§4.4); es el único código público que genera la macro además de los handlers. |
| RF-057 | Los extractores implementan `rdd::http::FromRequest` (`async fn from_request(ctx: &Ctx, bind: Bind) -> Result<Self>`): [`Ctx`](§4.4) envuelve la petición ya leída (RP-002) y `Bind` es la pista de enlace que pasa la macro —qué parámetro de ruta o qué cabecera—, necesaria porque la firma es única para todos los extractores. Como el cuerpo está en memoria, varios extractores pueden leerlo en la misma petición. La macro desenrolla la llamada a cada extractor en el cuerpo generado, sin límite de parámetros impuesto por el sistema de tipos. |

### 8.3 Cambio respecto a borradores previos

En borradores anteriores `impl TaskController` aparecía sin declarar el struct. Una macro de atributo sobre un `impl` no puede declarar el tipo, por lo que **el struct se declara explícitamente** y el scaffold lo genera.

### 8.4 `#[rdd::main]`

```rust
#[rdd::main]
async fn main() -> Result<()> { App::new().module(tasks::module()).run("0.0.0.0:3000").await }
```

Equivale a un `#[tokio::main]` más: `dotenvy::dotenv().ok()`, inicialización de `tracing_subscriber` leyendo `RUST_LOG` (por defecto `info`), y conversión del `Result` final en código de salida.

### 8.5 Expansión de referencia de un handler

`#[controller]` emite, por cada handler, una función `__rdd_route_<handler>()` que
devuelve la `RouteDef` (RF-056), y un `routes()` público que las agrupa. La
composición de middlewares, la conversión `Result<T>` → respuesta y el aislamiento
de pánicos no se escriben aquí: los hace el núcleo a través de
[`rdd::api::route_def`], que es el contrato estable entre macro y núcleo (RA-010
impide que `rdd-macros` dependa de `rdd-core`).

```rust
// #[controller("/tasks")] impl TaskController { #[get("/")]
//     async fn list(svc: TaskService) -> Result<Vec<Task>> { ... } }
impl TaskController {
    #[doc(hidden)]
    #[allow(non_snake_case)]
    pub fn __rdd_route_list() -> ::rdd::http::RouteDef {
        ::rdd::api::route_def(
            ::rdd::http::Method::GET,
            "/tasks",                       // prefijo + ruta, normalizada (RF-051)
            ::rdd::http::StatusCode::OK,    // estado base según verbo/unidad (RF-053)
            ::std::vec![],                  // #[middleware] de la ruta, en orden (§11.3)
            |__rdd_ctx: ::rdd::http::Ctx| async move {
                // RF-057: un `let` por parámetro, sin límite de aridad.
                let svc = ::rdd::api::extract::<TaskService>(&__rdd_ctx, ::rdd::http::Bind::none()).await?;
                TaskController::list(svc).await
            },
        )
    }

    /// RF-056: el único código público generado además de los handlers.
    pub fn routes() -> ::std::vec::Vec<::rdd::http::RouteDef> {
        ::std::vec![TaskController::__rdd_route_list()]
    }
}

// Registro con el que `module!` y `App::controller` montan el controller (§6.2).
impl ::rdd::Controller for TaskController {
    const NAME: &'static str = "TaskController";
    fn register(__rdd_router: &mut ::rdd::api::Router) {
        __rdd_router.push(TaskController::__rdd_route_list());
    }
}

impl ::rdd::ModuleMember for TaskController {
    fn register(__rdd_builder: ::rdd::ModuleBuilder) -> ::rdd::ModuleBuilder {
        __rdd_builder.controller::<TaskController>()
    }
}
```

Tres detalles frente a los borradores de esta sección (registro de cambios 0.2.1):

- `Ctx` es **propietario** (`Ctx::of(req)`), no un borrow: los extractores devuelven
  datos con vida propia y el futuro del closure debe poder enviarse a otro task
  (RF-073). `ctx.request()` da el borrow cuando se necesita.
- El estado base es un `StatusCode` que `route_def` aplica solo en `Ok`; un `Err`
  se materializa en Problem Details con el estado del error (§10.3, RP-004).
- `TaskService` como extractor llega con `#[service]` (§6.3, fase 0.2). En fase 0.1
  la misma forma aplica a `Json<T>`, `Path<T>`, `Query<T>`, `Header<T>` y `Ext<T>`,
  donde `Bind` lleva el nombre del parámetro de ruta o de la cabecera.

### 8.6 Restricciones

| ID | Requisito |
|---|---|
| RN-010 | Las macros NO DEBEN requerir `nightly`. |
| RN-011 | Las macros NO DEBEN usar `unsafe`. |
| RN-012 | El código generado DEBE usar rutas absolutas (`::rdd::...`) para evitar colisiones de nombres. |
| RN-013 | Las macros DEBERÍAN poder inspeccionarse con `cargo expand` y el resultado DEBE coincidir con §8.5. |

### 8.7 Catálogo de diagnósticos

| Código | Causa | Mensaje (resumen) |
|---|---|---|
| RDD0101 | Aggregate sin `#[id]` | "`#[aggregate]` necesita un campo marcado con `#[id]`" |
| RDD0102 | Más de un `#[id]` | "solo un campo puede ser `#[id]`" |
| RDD0201 | `#[service]` con campo de tipo no inyectable | "el campo `x: T` no puede inyectarse; registra `T` con `.provide()` o conviértelo en `#[service]`" |
| RDD0301 | Método de controller sin atributo de verbo | "añade `#[get]`, `#[post]`... o elimina el método del bloque `#[controller]`" |
| RDD0302 | Método con `self` | "los handlers son funciones asociadas, quita `self`" |
| RDD0303 | Ruta con sintaxis inválida | "la ruta debe empezar por `/`" |
| RDD0304 | Dos handlers con mismo verbo y ruta | "ruta duplicada: `GET /tasks`" |
| RDD0305 | Tipo de retorno no es `Result<T>` | "los handlers deben devolver `rdd::Result<T>`" |
| RDD0401 | `#[middleware]` con expresión que no implementa `Middleware` | "`X` no implementa `rdd::Middleware`" |

Cada diagnóstico DEBE señalar el span exacto y, cuando aplique, sugerir la corrección (`help:`).

---

## 9. Configuración tipada

| ID | Requisito |
|---|---|
| RF-060 | `Config<T>` donde `T: Deserialize` DEBE cargarse de `rdd.toml` (sección `[app.config]`) con sobreescritura por variables de entorno. |
| RF-061 | Precedencia: **variable de entorno > `rdd.toml` > valor por defecto**. |
| RF-062 | Variables de entorno con prefijo `RDD_` y separador `__` (ej. `RDD_DATABASE__URL`). |
| RF-063 | Los secretos NO DEBEN guardarse en `rdd.toml`; el scaffold añade `.env` a `.gitignore`. |

---

## 10. Capa HTTP

### 10.1 Contratos

| ID | Requisito |
|---|---|
| RF-070 | Todas las respuestas de éxito con cuerpo son `application/json`. |
| RF-071 | Los nombres de campos JSON siguen el nombre del campo Rust (snake_case) salvo que el usuario use `#[serde(rename_all = "camelCase")]`. |

### 10.2 Sintaxis de parámetros de ruta

El framework expone **siempre** la sintaxis `/:id` (RP-003). Cada plataforma la traduce a la de su router (por ejemplo, las versiones recientes de Axum usan llaves). La traducción es interna a la plataforma, el código de usuario no cambia al actualizar ni al cambiar de plataforma, y la suite de conformidad la verifica (C-04). ⚠️ V1.

### 10.3 Formato de errores (RFC 9457, Problem Details)

Todas las respuestas de error DEBEN usar `Content-Type: application/problem+json`:

```json
{
  "type": "about:blank",
  "title": "Unprocessable Entity",
  "status": 422,
  "detail": "el título no puede estar vacío",
  "instance": "/tasks"
}
```

| `Error` | HTTP |
|---|---|
| `BadRequest` | 400 |
| `Unauthorized` | 401 |
| `Forbidden` | 403 |
| `NotFound` | 404 |
| `Conflict` | 409 |
| `Invalid` | 422 |
| `Internal` | 500 (con `detail` genérico) |

| ID | Requisito |
|---|---|
| RF-072 | Rutas inexistentes DEBEN responder 404 en este formato; métodos no permitidos, 405 con cabecera `Allow`. |
| RF-073 | Un `panic` en un handler DEBE capturarse y devolver 500 sin tumbar el servidor. |

---

## 11. Sistema de middlewares

### 11.1 Modelo

Un middleware es una función asíncrona sobre los tipos del núcleo (`Request`, `Next`, `Response`) que devuelve una respuesta o un `Error`. El núcleo **compone la cadena completa** antes de entregar el handler a la plataforma (RP-005), por lo que el orden y la semántica son idénticos en cualquier plataforma.

```rust
pub trait Middleware: Send + Sync + 'static {
    fn handle(&self, req: Request, next: Next)
        -> impl Future<Output = Result<Response>> + Send;
}
```

**Compatibilidad con Tower:** el trait `Middleware` NO depende de Tower. Las capas Tower existentes se pueden usar **solo** con `AxumPlatform` y envuelven por fuera toda la cadena de `rdd` (RF-089).

### 11.2 Formas de registro

```rust
// A. Global
App::new()
    .middleware(Cors::permissive())
    .middleware(Logger::default())

// B. Función (sin trait ni boilerplate)
.middleware(fn_middleware(|req, next| async move {
    tracing::info!("{} {}", req.method(), req.uri());
    next.run(req).await
}))

// C. Capa Tower: es una opción de la plataforma Axum, no de App (RF-089)
//    .run_on(AxumPlatform::new().layer(CompressionLayer::new()), addr)

// D. Por controller o por ruta
#[controller("/tasks")]
#[middleware(Authenticated)]
impl TaskController {
    #[delete("/:id")]
    #[middleware(RequireRole::new("admin"))]
    async fn remove(svc: TaskService, id: Path<Uuid>) -> Result<()> { svc.remove(*id).await }
}
```

### 11.3 Orden de ejecución

El primer middleware registrado es el **más externo**.

```
Request ─► [globales en orden de registro] ─► [de módulo] ─► [de controller] ─► [de ruta] ─► handler
Response ◄──────────────────────────── (el mismo camino a la inversa) ◄─────────────────────────┘
```

| ID | Requisito |
|---|---|
| RF-080 | El orden anterior DEBE respetarse y estar cubierto por un test (§21). |
| RF-081 | Los middlewares globales DEBEN envolver también las respuestas 404/405 (fallback), para que CORS y logging cubran rutas inexistentes. |
| RF-082 | Un middleware PUEDE cortocircuitar devolviendo `Err(Error::...)` o una `Response` sin llamar a `next`. |
| RF-083 | Un middleware PUEDE pasar datos a los handlers con `req.extensions_mut().insert(valor)`; el handler los lee con `Ext<T>`. |
| RF-084 | Un middleware PUEDE obtener servicios con `req.resolve::<T>()` (usa el contenedor, §6.3). |
| RF-085 | `CORS` DEBE registrarse primero. Debe responder el preflight `OPTIONS` antes de cualquier autenticación. |
| RF-089 | Las capas Tower se configuran en la plataforma (`AxumPlatform::new().layer(...)`), envuelven por fuera toda la cadena de middlewares de `rdd` y NO pueden aplicarse por módulo, controller ni ruta. Las demás plataformas NO están obligadas a soportarlas. |

### 11.4 `#[middleware(expr)]`

| ID | Requisito |
|---|---|
| RF-086 | `expr` es cualquier expresión Rust que produzca un valor `impl Middleware`. |
| RF-087 | Múltiples `#[middleware]` en el mismo elemento se aplican en el orden en que aparecen (el primero es el más externo). |
| RF-088 | Un middleware de ruta NO DEBE ejecutarse si la ruta no hace match (no afecta a 404). |

### 11.5 Crate `rdd-cors`

Implementación **propia** sobre el trait `Middleware` del núcleo (sin Tower), de modo que funciona igual en cualquier plataforma. Sigue el protocolo CORS del estándar Fetch.

```rust
Cors::permissive()                          // desarrollo: cualquier origen
Cors::new()
    .allow_origin("https://miapp.com")
    .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
    .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION])
    .allow_credentials(true)
    .max_age(Duration::from_secs(3600))
Cors::from_config()                         // lee [cors] de rdd.toml
```

| ID | Requisito |
|---|---|
| RF-090 | `Cors::new()` sin configuración DEBE denegar todo origen cruzado (seguro por defecto). |
| RF-091 | `Cors::permissive()` DEBE emitir un `tracing::warn!` en perfil `release`. |
| RF-092 | `allow_credentials(true)` combinado con origen comodín (`*`) DEBE rechazarse en la construcción con error claro (la especificación CORS lo prohíbe). |
| RF-093 | `from_config()` lee `allow_origins`, `allow_methods`, `allow_headers`, `allow_credentials`, `max_age_secs` de la sección `[cors]`. |
| RF-094 | El preflight `OPTIONS` DEBE responder 204 sin ejecutar middlewares internos ni handlers. |
| RF-095 | `rdd-cors` DEBE depender solo de `rdd-core` (RA-011). |
| RF-096 | **Preflight** = `OPTIONS` con cabeceras `Origin` y `Access-Control-Request-Method`. Si el origen está permitido, DEBE responder 204 con `Access-Control-Allow-Origin`, `Access-Control-Allow-Methods`, `Access-Control-Allow-Headers`, `Access-Control-Max-Age` (y `Access-Control-Allow-Credentials` si aplica), más `Vary: Origin, Access-Control-Request-Method, Access-Control-Request-Headers`. |
| RF-097 | **Petición normal** con origen permitido: DEBE añadir `Access-Control-Allow-Origin` (el origen reflejado, o `*` solo si no hay credenciales), `Vary: Origin` y, si está configurado, `Access-Control-Expose-Headers`. |
| RF-098 | **Origen no permitido:** NO DEBE añadir cabeceras CORS ni bloquear la petición (el bloqueo lo ejecuta el navegador). Un preflight de origen no permitido responde sin cabeceras CORS. |
| RF-099 | Las peticiones sin cabecera `Origin` no son CORS y DEBEN pasar intactas. |

### 11.6 Crate `rdd-logger`

| ID | Requisito |
|---|---|
| RF-100 | Por cada petición DEBE emitir un evento `tracing` con: método, ruta, estado, latencia en ms y `request_id`. |
| RF-101 | DEBE generar `x-request-id` (UUID) si la petición no lo trae, propagarlo a la respuesta y añadirlo al span. |
| RF-102 | El nivel DEBE ser `INFO` para 2xx/3xx, `WARN` para 4xx y `ERROR` para 5xx. |
| RF-103 | NO DEBE registrar cuerpos ni cabeceras `Authorization`/`Cookie`. |
| RF-104 | Formato configurable: `Logger::default()` (legible) o `Logger::json()`. |

---

## 12. Persistencia

La persistencia es **multi-motor vía adaptadores**, con el mismo contrato que las
plataformas de §4.4: el núcleo define el puerto y cada motor lo implementa en su
propio crate. ADR-16 sustituye a ADR-10.

### 12.1 El puerto `DbAdapter` y `Repo<T>`

`Repo<T>` no cambia de cara al usuario: es un handle con los seis métodos.

```rust
impl<T: Aggregate + Persist> Repo<T>
where
    T::Id: Into<Value>,
{
    pub fn new(adapter: Arc<dyn DbAdapter>) -> Self;
    pub async fn get(&self, id: T::Id) -> Result<T>;               // NotFound si no existe
    pub async fn find(&self, id: T::Id) -> Result<Option<T>>;
    pub async fn all(&self) -> Result<Vec<T>>;
    pub async fn save(&self, aggregate: &T) -> Result<()>;         // upsert por id
    pub async fn delete(&self, id: T::Id) -> Result<()>;           // NotFound si no existe
    pub async fn exists(&self, id: T::Id) -> Result<bool>;
}
```

La cota `T::Id: Into<Value>` es necesaria porque `Aggregate::Id` (§7.1) es un tipo
asociado libre y el puerto solo habla `Value`. El núcleo provee `From` para `Uuid`,
`String`, `&str`, `bool`, `i64` e `i32`; un identificador exótico necesita su propio
`From`. `Repo<T>` implementa `FromContainer` (§6.3), así que `#[service]` lo inyecta
sin registro por agregado: lee el valor `Db` que `App::build()` deja en el contenedor.

```rust
/// Adaptador activo, inyectable por tipo (RF-022). Lo registra `App::build()`.
pub struct Db(pub Arc<dyn DbAdapter>);
```

Por debajo habla con un puerto object-safe. El núcleo no conoce ningún driver
(RA-014): solo este trait, un valor neutro y una fila neutra.

```rust
// rdd-core
pub enum Value {
    Null, Bool(bool), I64(i64), F64(f64),
    Text(String), Bytes(Vec<u8>), Uuid(uuid::Uuid), Json(serde_json::Value),
}

/// Fila neutra: columna → valor. Conserva el orden de inserción, pero el orden no
/// es contractual: `from_row` lee por nombre.
pub struct Row { /* … */ }

pub trait DbAdapter: Send + Sync + 'static {
    fn name(&self) -> &'static str;
    fn get<'a>(&'a self, table: &'a str, id_column: &'a str, id: &'a Value)
        -> BoxFuture<'a, Result<Option<Row>>>;
    fn all<'a>(&'a self, table: &'a str) -> BoxFuture<'a, Result<Vec<Row>>>;
    fn save<'a>(&'a self, table: &'a str, columns: &'a [&'a str], values: &'a [Value])
        -> BoxFuture<'a, Result<()>>;
    fn delete<'a>(&'a self, table: &'a str, id_column: &'a str, id: &'a Value)
        -> BoxFuture<'a, Result<bool>>;
}
```

La vida `'a` es explícita y común a todos los argumentos: con `BoxFuture<'_>` el
elisión la ata solo a `&self`, y el `async move` del adaptador no podría capturar
`table` ni `id`. Sigue siendo object-safe.

`id_column` va explícito en `get` y `delete` porque un motor SQL necesita el nombre
de la columna para el `WHERE` y no puede adivinarlo: `Persist::COLUMNS[0]` puede
llamarse `task_id`, no `id`. `Repo<T>` lo pasa desde `T::COLUMNS[0]`; los motores
documentales y KV lo ignoran (usan `_id` o la clave). `all` no recibe columnas y
hace `SELECT *`, así que **el orden de `Row` no es contractual**: `from_row` lee
por nombre y las columnas de más se ignoran.

El puerto tiene **cuatro** operaciones, no seis: `find`/`get` se distinguen en
`Repo<T>` (una devuelve `Option`, la otra `NotFound`) y `exists` se resuelve con
`get`. Así cada motor implementa menos y hay menos superficie que portar.

En `save`, `columns[0]` y `values[0]` SON el identificador; es la convención que
permite el upsert sin firmar un método por motor.

| ID | Requisito |
|---|---|
| RF-110 | `Repo<T>` es un **handle** sobre `Arc<dyn DbAdapter>`. `MemoryBackend` vive en el núcleo (no arrastra driver); cada motor vive en su crate. |
| RF-111 | El adaptador se elige al arrancar por el **esquema de la URL**: `sqlite://`, `postgres://`/`postgresql://`, `mysql://`/`mariadb://`, `mongodb://`, `redis://`, `sqlserver://`. SQLite se incluye por defecto en la fachada `rdd`; `.in_memory()` fuerza memoria. Un esquema cuyo adaptador no está instalado DEBE fallar **al arrancar** diciendo qué crate añadir, nunca en tiempo de petición. |
| RF-112 | `save` DEBE ser un upsert atómico en una sola operación nativa del motor (§12.5). |
| RF-113 | Toda consulta DEBE usar parámetros enlazados; NO DEBE interpolarse texto del usuario. En motores sin parámetros (Redis) la clave se compone solo con el identificador saneado. |
| RF-114 | Los identificadores DEBEN entrecomillarse con la convención del motor (§12.5). El usuario nunca escribe comillas de SQL a mano. |
| RF-115 | Para consultas especiales un service PUEDE inyectar el handle nativo del adaptador (`Db`, p. ej. `sqlx::PgPool`) y usar el cliente directamente. Ese código es específico del motor y no portable. |
| RF-116 | El pool DEBE configurarse en `rdd.toml` (`max_connections`, `acquire_timeout_secs`). Los motores sin pool propio (MSSQL) lo añaden dentro de su adaptador. |
| RF-117 | `.run()` NO DEBE aplicar migraciones automáticamente. Las migraciones son un paso explícito (`rdd db migrate deploy`). |

### 12.2 `Persist`

Neutro: ya no recibe una fila de PostgreSQL.

```rust
pub trait Persist: Aggregate + Sized {
    const TABLE: &'static str;
    const COLUMNS: &'static [&'static str];
    fn to_values(&self) -> Vec<Value>;
    fn from_row(row: &Row) -> Result<Self>;
}
```

Lo implementa `#[aggregate(table = ...)]` (RF-032) o, con `row = ...`, el código
generado en `src/generated/` (§13.6). `COLUMNS` y `to_values` DEBEN tener la misma
longitud y el mismo orden, con el identificador primero.

Los motores documentales y KV no consultan por columnas: serializan el agregado
completo con `serde`. `Persist` sigue siendo el contrato común porque `TABLE`
(colección o prefijo de clave) y el identificador son universales.

### 12.3 Transacciones y concurrencia

Fuera de alcance en 0.x. Cada `save` es autocommit. La concurrencia optimista
(columna `version`) y `#[transactional]` son candidatos posteriores. El puerto NO
expone transacciones: exigirlas obligaría a todos los motores a soportarlas, y
Redis no las ofrece como un SQL.

### 12.4 Suite de conformidad (`rdd-db-tests`)

Mismo razonamiento que ADR-15 para las plataformas: sin una suite compartida, el
puerto acaba copiando la forma del primer adaptador escrito.

| ID | Requisito |
|---|---|
| RF-180 | Todo adaptador DEBE pasar `rdd-db-tests`: los seis métodos de `Repo<T>`, upsert que actualiza sin duplicar, `NotFound` en `get`/`delete` de un id inexistente, `all` con tabla vacía, y round-trip de cada variante de `Value` que el motor soporte. |
| RF-181 | La suite DEBE poder correr contra memoria sin servidor, y contra cada motor real cuando haya instancia. CI DEBE distinguirlas: un fallo de conexión no es un fallo de conformidad. |
| RF-182 | Un motor que no soporte una variante de `Value` DEBE declararlo en su adaptador, y la suite DEBE saltar ese caso con razón explícita, nunca pasar en silencio. |

El test de integración de cada adaptador se activa con `RDD_TEST_<MOTOR>_URL`
(`RDD_TEST_POSTGRES_URL`, `RDD_TEST_MYSQL_URL`, …). Sin la variable el test se
salta anunciándolo; con ella, un fallo de conexión es un fallo del test, no un
salto: así CI distingue «no hay servidor» de «el motor no cumple el contrato»
(RF-181). El crate del motor crea y destruye su propia tabla de sonda con su
cliente nativo, porque el puerto no tiene DDL (RF-117).

### 12.5 Semántica por motor

| Motor | Crate | Upsert (RF-112) | Identificadores (RF-114) | Notas |
|---|---|---|---|---|
| Memoria | `rdd-core` | reemplazo por id | — | Sin driver. Backend de tests y de `rdd run --memory` (RF-015). |
| SQLite | `rdd-sqlite` | `INSERT … ON CONFLICT (id) DO UPDATE` | `"tabla"."columna"` | `sqlx`. Adaptador predeterminado; crea el archivo local si no existe. Las columnas UUID y JSON se declaran `UUID` y `JSON` para conservar sus variantes. |
| PostgreSQL | `rdd-postgres` | `INSERT … ON CONFLICT (id) DO UPDATE` | `"tabla"."columna"` | `sqlx`. Referencia del puerto. Solo decodifica tipos con variante simétrica en `Value` (RF-187). |
| MySQL/MariaDB | `rdd-mysql` | `INSERT … ON DUPLICATE KEY UPDATE` | `` `tabla`.`columna` `` | `sqlx`. Exige clave primaria o única en `id`. |
| MongoDB | `rdd-mongodb` | `replaceOne` con `upsert: true` | no aplica | Documento completo por `_id`. `all` = `find({})`. |
| Redis | `rdd-redis` | `SET clave valor` | no aplica | KV. `all` = `SCAN`: **O(N), no apto para producción** con muchas claves. Sin consultas por campo. |
| SQL Server | `rdd-mssql` | `MERGE` | `[tabla].[columna]` | `tiberius-ng` + `bb8`. Necesita servidor real (RA-016). |

Redis tiene además un papel que no es el de repositorio: caché, sesiones y rate
limiting en `infrastructure/cache/` (§5.1). Son dos usos del mismo crate, no dos
crates.

| ID | Requisito |
|---|---|
| RF-183 | `rdd-redis` DEBE documentar que `all()` es un `SCAN` y NO DEBE presentarse como equivalente a un `SELECT` sin límites. |
| RF-184 | El adaptador de Redis para agregados y la API de caché (TTL, sesiones) DEBEN ser distinguibles en la API; un `Repo<T>` sobre Redis no aplica TTL implícito. |
| RF-185 | Añadir un motor nuevo NO DEBE requerir cambios en `rdd-core` ni en los demás adaptadores. Si los requiere, el puerto está mal diseñado. |
| RF-186 | Prisma (§13) sigue generando solo para PostgreSQL. Un proyecto sobre otro motor DEBE poder usar `Repo<T>` sin `schema.prisma`, y `rdd db generate` DEBE avisar de que no aplica a ese motor en vez de generar código inválido. |
| RF-187 | Cada adaptador DEBE limitar la decodificación a los tipos nativos con variante equivalente en `Value`, de modo que leer y volver a guardar sea simétrico. Un tipo fuera de esa lista (en PostgreSQL: `TIMESTAMPTZ`, `TIMESTAMP`, `DATE`, `NUMERIC`) DEBE producir un error que nombre la columna y el tipo y remita a **V15**, nunca convertirse a texto ni perderse en silencio: ligar un `String` envía el OID de `TEXT` y la columna lo rechaza al guardar. |

### 12.6 Registro de adaptadores y arranque

RA-014 prohíbe al núcleo conocer un driver, así que el núcleo tampoco puede saber
qué crate implementa `mongodb://`. La solución es la misma que RP-012 usa para la
plataforma por defecto: **un registro global en el núcleo que la fachada puebla**.

```rust
// rdd-core::db
pub type AdapterFactory = fn(String) -> BoxFuture<'static, Result<Arc<dyn DbAdapter>>>;

/// Declara un motor conocido aunque no esté instalado: da el mensaje de RF-111.
pub fn declare(schemes: &[&'static str], crate_name: &'static str, feature: &'static str);
/// Instala la factoría de un motor compilado en la fachada.
pub fn install(scheme: &str, factory: AdapterFactory);
/// Resuelve el esquema de la URL; error de arranque si no hay factoría.
pub fn factory_for(url: &str) -> Result<AdapterFactory>;
```

`declare` la llama la fachada para los seis motores, con o sin feature; `install`
solo para los compilados. Así el mensaje de un esquema no instalado nombra crate y
feature exactos, y **añadir un motor nuevo no toca el núcleo** (RF-185): basta su
crate y una línea en la fachada.

| ID | Requisito |
|---|---|
| RF-118 | `App::build()` DEBE seguir siendo **síncrono**: compone rutas, middlewares y contenedor sin I/O (RF-012). Con una URL pendiente de conectar DEBE fallar diciendo que se llame a `App::connect().await` o se use `.run()`. |
| RF-119 | `App::connect(self) -> Result<Self>` DEBE resolver la URL por el registro, conectar y dejar el adaptador en el builder. `.run()`/`.run_on()` lo llaman antes de componer, así que un esquema sin adaptador o una conexión rechazada fallan **antes** de abrir el socket (RF-111). `.adapter(Arc<dyn DbAdapter>)` inyecta uno ya conectado y salta el registro. |

---

## 13. Integración con Prisma

### 13.1 Decisión

El cliente `prisma-client-rust` **no se usa**: su repositorio principal indica que no recibe mantenimiento y su última versión soportada de Prisma es antigua. Se usa **Prisma únicamente como herramienta de esquema y migraciones**, y el adaptador del motor como cliente en tiempo de ejecución (ADR-03, ADR-16).

**Prisma sigue siendo solo PostgreSQL** (RF-186). El parser (§13.4), el mapeo de
tipos (§13.5) y los enums (§13.6) están escritos para PostgreSQL. Un proyecto
sobre MySQL, MongoDB, Redis o MSSQL usa `Repo<T>` sin `schema.prisma` y crea el
esquema con la herramienta nativa de su motor; unificar migraciones multi-motor
es una fase aparte, no un subproducto de los adaptadores.

```
schema.prisma ──► prisma migrate ──► prisma/migrations/*.sql        (Prisma CLI)
      │
      └────────► rdd db generate ──► src/generated/*.rs             (rdd-prisma)
                                      structs de fila + impl Persist neutro (§12.2)
```

### 13.2 Requisitos de entorno

| ID | Requisito |
|---|---|
| RN-020 | La CLI de Prisma DEBE ejecutarse con una **versión fijada** en `rdd.toml` (`[prisma] cli_version`), invocada vía `npx prisma@<versión>`. |
| RN-021 | Node.js es requisito **solo de desarrollo**. El binario de producción NO DEBE depender de Node ni de Prisma. |
| RN-022 | `rdd doctor` DEBE verificar: Node disponible, Prisma CLI resoluble, `DATABASE_URL` definida, conexión a la base de datos. |

⚠️ Las versiones recientes de Prisma han movido ciertos ajustes (como la URL de conexión) a un archivo de configuración propio (`prisma.config.ts`). Al iniciar la fase 0.4 se DEBE verificar qué estructura exige la versión fijada y reflejarlo en la plantilla de `rdd new`.

### 13.3 Comandos

| Comando `rdd` | Ejecuta |
|---|---|
| `rdd db migrate dev --name <n>` | `prisma migrate dev --name <n>`, y luego `rdd db generate`. |
| `rdd db migrate deploy` | `prisma migrate deploy` (producción/CI). |
| `rdd db migrate status` | `prisma migrate status`. |
| `rdd db reset` | `prisma migrate reset` (pide confirmación; rechaza ejecutarse si `RDD_ENV=production`). |
| `rdd db generate` | Parsea `schema.prisma` y emite `src/generated/`. |
| `rdd db check` | Comprueba que `src/generated/` está al día y que los aggregates coinciden con el esquema (§13.7). |

### 13.4 Gramática soportada de `schema.prisma` (subconjunto)

| ID | Requisito |
|---|---|
| RF-120 | El parser DEBE soportar: bloques `datasource`, `generator` (se ignoran), `model` y `enum`. |
| RF-121 | Campos con tipos escalares: `String`, `Boolean`, `Int`, `BigInt`, `Float`, `Decimal`, `DateTime`, `Json`, `Bytes`, y tipos `enum` declarados. |
| RF-122 | Modificadores de tipo: opcional `?` y lista `[]`. |
| RF-123 | Atributos de campo: `@id`, `@default(...)`, `@unique`, `@map("...")`, `@updatedAt`, `@db.Uuid` (y otros `@db.*` que afecten al tipo Rust). |
| RF-124 | Atributos de modelo: `@@map("...")`, `@@id([...])`, `@@unique([...])`, `@@index([...])`. |
| RF-125 | Relaciones (`@relation`): en 0.4 se reconocen para **leer la clave foránea** (campo escalar) pero NO se generan métodos de carga relacional. |
| RF-126 | Cualquier construcción no soportada DEBE producir error con archivo, línea y columna, nunca ignorarse en silencio; salvo bloques/atributos de la lista de ignorados. |
| RF-127 | Los comentarios `//` y `///` DEBEN ignorarse. |

### 13.5 Mapeo de tipos (PostgreSQL)

| Prisma | Rust | Notas |
|---|---|---|
| `String` | `String` | |
| `String @db.Uuid` | `uuid::Uuid` | Recomendado para ids. |
| `Boolean` | `bool` | |
| `Int` | `i32` | |
| `BigInt` | `i64` | |
| `Float` | `f64` | |
| `Decimal` | `rust_decimal::Decimal` | Requiere la feature correspondiente de SQLx. |
| `DateTime` | `chrono::DateTime<Utc>` | |
| `Json` | `serde_json::Value` | |
| `Bytes` | `Vec<u8>` | |
| `enum X` | `enum X` con `sqlx::Type` | |
| `T?` | `Option<T>` | |
| `T[]` | `Vec<T>` | Solo escalares/enums soportados por PostgreSQL. |

### 13.6 Código generado

Para cada `model` se genera en `src/generated/rows.rs`:

```rust
// Generado por `rdd db generate`. NO EDITAR.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct TaskRow {
    pub id: Uuid,
    pub title: String,
    pub done: bool,
    pub created_at: DateTime<Utc>,
}

impl ::rdd::Table for TaskRow {
    const TABLE: &'static str = "tasks";
    const COLUMNS: &'static [&'static str] = &["id", "title", "done", "created_at"];
}
```

| ID | Requisito |
|---|---|
| RF-130 | Los nombres Rust DEBEN ser: modelo → `PascalCase` + sufijo `Row`; campo → `snake_case`. |
| RF-131 | Los nombres SQL DEBEN respetar `@map`/`@@map`. Sin ellos, se usa el nombre tal cual lo crea Prisma (modelo y campo con su capitalización original, siempre entre comillas, RF-114). |
| RF-132 | `src/generated/` DEBE empezar con un encabezado de "archivo generado" y DEBERÍA incluirse en git para que `cargo build` funcione sin Node. |
| RF-133 | La generación DEBE ser **determinista** (mismo esquema → mismos bytes) para que los diffs sean limpios. |
| RF-134 | Los enums generados DEBEN mapear a los tipos `ENUM` de PostgreSQL con el nombre que Prisma les da. |

### 13.7 Verificación aggregate ↔ esquema

`rdd db check` DEBE comprobar que cada `#[aggregate(table = "t")]` tiene todos sus campos como columnas de `t` con tipo compatible, y reportar discrepancias con nombre de campo y columna. Es análisis estático de mejor esfuerzo; el error definitivo sigue siendo el de SQLx en ejecución.

### 13.8 Trampas conocidas de Prisma que el framework DEBE manejar

| Comportamiento de Prisma | Consecuencia | Tratamiento en `rdd` |
|---|---|---|
| `@default(uuid())` y `@default(cuid())` los genera el **cliente Prisma**, no la base de datos. | Un `INSERT` desde SQLx no recibe valor por defecto. | El aggregate genera su propio id (`Uuid::new()`). La documentación recomienda `@default(dbgenerated("gen_random_uuid()")) @db.Uuid` si se quiere default en BD. |
| `@updatedAt` lo calcula el cliente Prisma. | La BD no actualiza la columna. | `Repo::save` DEBE asignar `now()` a las columnas `@updatedAt` en cada upsert. |
| Sin `@@map`, la tabla se llama como el modelo (`"Task"`, con mayúscula). | Consultas rotas si se escribe a mano `tasks`. | La plantilla del scaffold usa `@@map` y `@map` en snake_case (§15.2). |
| `@default(now())` sí crea un default en BD. | | Sin tratamiento especial. |

### 13.9 Alternativa documentada (no implementar en 0.4)

Generar el código desde la **introspección de la BD** (`information_schema`) tras aplicar migraciones, en lugar de parsear `schema.prisma`. Ventaja: no hay parser. Desventaja: se pierden semánticas de Prisma (`@updatedAt`, defaults de cliente). Se evaluará si el mantenimiento del parser resulta costoso (ADR-04).

---

## 14. CLI `rdd`

### 14.1 Comandos

| Comando | Descripción |
|---|---|
| `rdd new <name>` | Crea el proyecto con el árbol completo de §5 (RF-002) y el módulo de ejemplo `tasks`, desactivable con `--no-example`. |
| `rdd g module <name>` | Genera un módulo con la convención NestJS de §5 (RF-001). |
| `rdd g middleware <Name> [--module <m>]` | Genera un middleware propio. |
| `rdd add <paquete>` | Ejecuta `cargo add` para `cors`, `logger`, o un crate externo y muestra el fragmento a añadir en `main.rs`. |
| `rdd migrate [--database-url <url>] [--source <dir>]` | Aplica las migraciones SQL pendientes antes del despliegue. Por defecto lee la URL desde la variable indicada por `[database].url_env` y el directorio desde `[database].migrations_dir` (`migrations/`). |
| `rdd run [--memory]` | Ejecuta la app; con `--memory` usa backend en memoria (sin base de datos). |
| `rdd dev` | Como `run`, recompilando al cambiar archivos (usa `cargo-watch` si está instalado; si no, indica cómo instalarlo). |
| `rdd db ...` | §13.3 |
| `rdd check` | Verifica reglas de arquitectura (§17) y dependencias (RF-027). |
| `rdd doctor` | Verifica el entorno (§13.2). |

`rdd migrate` lee `.env` y `rdd.toml`, aplica en orden ascendente los archivos
`<VERSION>_<DESCRIPCION>.sql` (VERSION entero positivo), y registra versiones y
checksums en `_sqlx_migrations`. Solo corre migraciones pendientes; una migración
ya aplicada cuyo contenido cambió produce un error. SQLite crea el archivo si no
existe; PostgreSQL requiere que la base ya exista. El comando soporta SQLite y
PostgreSQL, los drivers SQL disponibles actualmente. Los archivos SQL deben ser
compatibles con el motor configurado. No se ejecutan migraciones al arrancar la
app: CI/CD o el operador DEBE ejecutar `rdd migrate` como paso explícito antes
de `rdd run` o de poner la nueva versión en servicio. `--database-url` y
`--source` permiten sobreescribir los valores configurados.

| ID | Requisito |
|---|---|
| RF-188 | `rdd migrate` DEBE aplicar solo migraciones pendientes en orden, registrar versiones/checksums, fallar explícitamente ante errores o cambios de checksum y ser idempotente. El arranque del servidor NO DEBE migrar implícitamente; el despliegue DEBE poder ejecutar migraciones como paso separado. |

### 14.2 Archivos que genera `rdd g module tasks`

El árbol de módulo de §5, bajo `src/modules/tasks/`: `mod.rs` más las nueve
carpetas `controllers/`, `services/`, `repositories/`, `dto/`, `entities/`,
`interfaces/`, `mappers/`, `constants/` y `tests/`, cada una con su `mod.rs` y los
archivos nombrados en §5 (RF-001, RF-003, RF-005).

El contenido de partida es el mínimo que compila y responde (RF-006, RF-007):
`entities/` con el agregado en Rust plano, `dto/` con los tres DTOs,
`services/` con el CRUD mínimo, `controllers/` con `#[controller("/tasks")]` y
los cinco verbos (`list`, `get`, `create`, `update`, `remove`), y `tests/` con un
test por capa. En 0.2, `#[aggregate]` y `#[service]` sustituyen al Rust plano sin
cambiar el árbol.

Los nombres que genera el CLI, con `tasks` como ejemplo: `TasksController`,
`TasksService`, `TasksRepository` (la implementación), `TaskStore` (el puerto en
`interfaces/`), `Task` (el agregado), `TaskMapper`, `CreateTaskDto`,
`UpdateTaskDto`, `QueryTaskDto` y `MAX_TITLE_LEN`. El service depende del puerto,
nunca de la implementación (§5.1). `TasksRepository::shared()` es el almacén de
la app y `TasksRepository::new()` uno vacío y aislado, que es lo que usan los
tests del módulo para no compartir estado.

`tests/acceptance.rs` incluye `src/` por `#[path]` porque un test de integración
no puede importar el crate binario; así `crate::modules::…` y `crate::app_*`
resuelven igual que en `main.rs` y no hay que duplicar el cableado.

**Transición a 0.2.** En 0.1 `TasksRepository` es un `Vec` en memoria porque
todavía no hay persistencia (RF-006). En 0.2 pasa a envolver `Repo<Task>` sobre el
adaptador que decida `DATABASE_URL` (§12), y el puerto `TaskStore` se vuelve
**async**: `Repo<T>` lo es, y un puerto síncrono obligaría a bloquear el runtime.
El árbol de carpetas no cambia; cambian el contenido de `repositories/`, la firma
de `interfaces/` y los `await` de `services/`. `shared()`/`new()` desaparecen: el
aislamiento de los tests pasa a `MemoryBackend` o a `TestApp` (RF-160), no a un
estático global.

### 14.3 Marcadores de registro

`rdd g module` DEBE insertar el módulo en dos archivos, buscando un marcador de
comentario en cada uno. Si el marcador no existe, DEBE mostrar las líneas a pegar
a mano en vez de fallar.

`src/modules/mod.rs` — declara el módulo Rust:

```rust
// rdd:mods
pub mod tasks;
```

`src/main.rs` — lo monta en la app:

```rust
// El scaffold reserva carpetas para fases posteriores (RF-004).
#![allow(dead_code)]

use rdd::prelude::*;

mod app_controller;
mod app_module;
mod app_service;
mod common;
mod config;
mod generated;
mod infrastructure;
mod integrations;
mod modules;
mod shared;

#[rdd::main]
async fn main() -> Result<()> {
    let config = config::app_config::load();
    let addr = format!("{}:{}", config.host, config.port);

    let app = App::new()
        .config(config)
        // rdd:modules
        .module(modules::tasks::module())
        .module(app_module::module());

    // `rdd run --memory` deja `RDD_MEMORY=1` (RF-015).
    let app = if std::env::var("RDD_MEMORY").is_ok() {
        app.in_memory()
    } else {
        app
    };

    app.run(&addr).await
}
```

`rdd g module` inserta la línea nueva inmediatamente debajo del marcador, o sea
antes de `.module(app_module::module())`, y conserva el comentario. Si el
proyecto se generó con `--no-example`, el marcador va seguido directamente por
`.module(app_module::module())`.

La declaración vive en `src/modules/mod.rs` y no en `main.rs` porque con la
estructura de §5 los módulos son hijos de `modules/`; `main.rs` solo los monta.

| ID | Requisito |
|---|---|
| RF-140 | Todo comando que escribe archivos DEBE negarse a sobrescribir archivos existentes salvo `--force`. |
| RF-141 | `--dry-run` DEBE listar los archivos que se crearían/modificarían sin escribir. |
| RF-142 | Los nombres se normalizan: `Tasks`/`tasks`/`task-items` → módulo `snake_case`, tipos `PascalCase`. |
| RF-143 | Todo proyecto generado DEBE compilar (`cargo check`) sin cambios manuales. Esto es un test de aceptación (§21). |

---

## 15. Archivo `rdd.toml`

### 15.1 Esquema

```toml
[app]
name = "todo-app-ddd"
host = "0.0.0.0"
port = 3000
body_limit_bytes = 2_097_152

[database]
# El motor lo decide el esquema de la URL (RF-111). `provider` es orientativo:
# lo usa `rdd doctor` para saber qué crate de adaptador comprobar.
provider = "sqlite"            # sqlite | postgresql | mysql | mongodb | redis | mssql | memory
url_env  = "DATABASE_URL"      # nombre de la variable; nunca la URL (RF-063)
max_connections = 10
acquire_timeout_secs = 5

[prisma]
schema        = "prisma/schema.prisma"
cli_version   = "<fijar al iniciar fase 0.4>"
generated_dir = "src/generated"

[cors]                          # leído solo por rdd-cors (Cors::from_config)
allow_origins     = ["http://localhost:5173"]
allow_methods     = ["GET", "POST", "PUT", "PATCH", "DELETE"]
allow_headers     = ["content-type", "authorization"]
allow_credentials = false
max_age_secs      = 3600

[logger]                        # leído solo por rdd-logger
format = "pretty"               # "pretty" | "json"
```

### 15.2 Plantilla de `schema.prisma` que genera `rdd new`

```prisma
datasource db {
  provider = "postgresql"
}

model Task {
  id    String  @id @default(dbgenerated("gen_random_uuid()")) @db.Uuid
  title String
  done  Boolean @default(false)

  @@map("tasks")
}
```

Sin `createdAt`: hasta que **V15** se resuelva, `Value` no tiene variante temporal
y el código generado desde este esquema no podría leer la columna (RF-187). Es la
misma decisión que §25.5.

⚠️ La forma exacta del bloque `datasource` (con o sin `url`) depende de la versión de Prisma fijada (§13.2).

| ID | Requisito |
|---|---|
| RF-150 | Secciones desconocidas en `rdd.toml` DEBEN ignorarse (permite a paquetes de terceros añadir las suyas); claves desconocidas dentro de secciones del núcleo DEBEN producir advertencia. |
| RF-151 | Cada paquete externo DEBE leer solo su propia sección. |

---

## 16. Convenciones

| Elemento | Convención | Ejemplo |
|---|---|---|
| Módulo (carpeta) | `snake_case`, plural | `tasks/` |
| Aggregate | `PascalCase`, singular | `Task` |
| Service | `<Aggregate>Service` | `TaskService` |
| Controller | `<Aggregate>Controller` | `TaskController` |
| Fila generada | `<Modelo>Row` | `TaskRow` |
| Middleware | Sustantivo o adjetivo | `Authenticated`, `RequireRole` |
| Tabla SQL | `snake_case`, plural (vía `@@map`) | `tasks` |
| Columna SQL | `snake_case` (vía `@map`) | `created_at` |
| Rutas REST | Sustantivos en plural, sin verbos | `PUT /tasks/:id/complete` solo para acciones de dominio |

---

## 17. Reglas de arquitectura (`rdd check`)

El análisis es estático por archivo, basado en la **carpeta y el nombre del
archivo** dentro del módulo (§5), usando `syn` para leer sentencias `use`.

| ID | Archivo | NO DEBE importar |
|---|---|---|
| RA-001 | `modules/<m>/entities/` y `modules/<m>/interfaces/` | `axum`, cualquier driver (`sqlx`, `mongodb`, `redis`, `tiberius`), `rdd::http`, `services`, `controllers`, `repositories`, `generated` |
| RA-002 | `modules/<m>/services/` | `controllers`, `axum` |
| RA-003 | `modules/<m>/controllers/` | `generated`, cualquier driver, `Repo`, `repositories` (un controller no habla con el repositorio) |
| RA-004 | Un módulo `A` | Archivos internos de otro módulo `B`; solo puede usar su API pública (lo que `b/mod.rs` re-exporta con `pub use`) |

**RA-005.** Las violaciones DEBEN reportarse con archivo, línea, regla y sugerencia. `rdd check` retorna código distinto de cero si hay violaciones (apto para CI).

**RA-006.** Se permite suprimir una regla en una línea con `// rdd:allow(RA-003) motivo`; el motivo es obligatorio.

**RA-007.** `modules/<m>/repositories/` es la única carpeta que toca un adaptador o su driver. El service depende del puerto de `interfaces/` o de `Repo<T>`, nunca del crate del motor: cambiar de PostgreSQL a MongoDB no debe tocar un service (RF-185). La excepción es RF-115 (consultas especiales con el handle nativo), que DEBE suprimirse con `// rdd:allow(RA-007)` y motivo.

---

## 18. Seguridad (requisitos base)

| ID | Requisito |
|---|---|
| RN-030 | Seguro por defecto: CORS cerrado (RF-090), límite de cuerpo (RF-017), errores internos sin detalle (RF-041). |
| RN-031 | SQL siempre parametrizado (RF-113). |
| RN-032 | Los logs NO DEBEN contener secretos (RF-103). |
| RN-033 | `rdd db reset` y `Cors::permissive()` DEBEN advertir o negarse en producción. |
| RN-034 | El proyecto DEBE ejecutar `cargo audit` (o `cargo deny`) en CI. |

---

## 19. Observabilidad

| ID | Requisito |
|---|---|
| RN-040 | Se usa `tracing`; el nivel se controla con `RUST_LOG`. |
| RN-041 | Cada petición tiene un span con `request_id` (RF-101); los logs emitidos dentro del handler lo heredan. |
| RN-042 | Al arrancar, la app DEBE registrar: dirección de escucha, backend de persistencia, módulos y rutas montadas. |
| RN-043 | Métricas (Prometheus) y trazas distribuidas: fuera del alcance 0.x. |

---

## 20. Testing

| ID | Requisito |
|---|---|
| RF-160 | `TestApp::new(app)` DEBE ejecutar la app **sin abrir un puerto** (invocando `HttpApp::dispatch` del núcleo, RP-014) y exponer un cliente con `.get()`, `.post()`, `.put()`, `.delete()`, `.json()`, `.header()`. |
| RF-161 | `TestApp` DEBE usar el backend en memoria por defecto. |
| RF-162 | Los aggregates se prueban como código Rust normal, sin framework (son tipos de dominio puros). |
| RF-163 | `rdd g module` DEBE generar un test de integración de ejemplo por módulo en `tests/`. |
| RF-164 | `TestApp::on(plataforma)` DEBERÍA levantar la app con una plataforma real en un puerto efímero para pruebas de integración de extremo a extremo. |

```rust
#[tokio::test]
async fn crea_y_completa_una_tarea() {
    let app = TestApp::new(App::new().in_memory().module(tasks::module()));

    let r = app.post("/tasks").json(&json!({"title": "Comprar leche"})).send().await;
    assert_eq!(r.status(), 201);
    let id = r.json::<Value>()["id"].as_str().unwrap().to_owned();

    let r = app.put(&format!("/tasks/{id}/complete")).send().await;
    assert_eq!(r.status(), 200);
    assert_eq!(r.json::<Value>()["done"], true);

    let r = app.put(&format!("/tasks/{id}/complete")).send().await;
    assert_eq!(r.status(), 409);   // la regla del aggregate se refleja como Conflict
}
```

---

## 21. Fases, entregables y criterios de aceptación

### Fase 0.1 — Núcleo HTTP

**Entrega:** `rdd`, `rdd-macros`, `rdd-cli` (`new`, `g module`, `run`).

- [x] `#[rdd::main]`, `App`, `Module`, `module!`
- [x] Crates `rdd-core` + `rdd` (fachada) + `rdd-macros`; RA-011 verificada localmente (el núcleo no arrastra `axum` ni `hyper`); la verificación en CI queda en los criterios transversales
- [x] `HttpApp`, `RouteDef`, `Request`/`Response`, trait `Platform`, `HttpApp::dispatch` (§4.4)
- [x] `rdd-axum` como plataforma por defecto, con `AxumPlatform::layer`
- [x] `rdd-platform-tests` con los casos C-02 a C-08 y C-11 a C-12 (el resto llega con la fase 0.3)
- [x] `#[controller]` y los cinco verbos, con la expansión de §8.5
- [x] Extractores `Json`, `Path`, `Query`
- [x] `Error` y respuestas Problem Details (§10.3)
- [x] Apagado ordenado (RF-013), `/health` (RF-016), límite de cuerpo (RF-017), normalización de barra (RF-018)
- [x] Diagnósticos RDD0301–RDD0305
- [x] `rdd new`, `rdd g module`, `rdd migrate` y `rdd run` con el árbol de §5 (RF-001 a RF-007, RF-140, RF-141, RF-143, RF-188)

**Aceptación:** un proyecto creado con `rdd new` compila sin cambios (RF-143), responde en `/health` y devuelve 404 y 405 en formato Problem Details; un test de integración recorre todos los códigos de §10.3; `AxumPlatform` y `dispatch` pasan la suite de conformidad disponible en esta fase.

### Fase 0.2 — Dominio, DI y persistencia

**Entrega:** `#[aggregate]`, `#[service]`, puerto `DbAdapter`, `Repo<T>`, `Persist`, `MemoryBackend`, `rdd-db-tests`, `rdd-sqlite`, `rdd-postgres`, `rdd-mysql`.

- [ ] Puerto `DbAdapter` + `Value` + `Row` neutros en `rdd-core`, sin ningún driver (RA-014, RF-185)
- [ ] `Persist` (§12.2): trait del **aggregate** (`TABLE`, `COLUMNS`, `to_values`, `from_row`), lo implementa `#[aggregate]`. Un boceto previo lo definió como repositorio CRUD síncrono con backends MySQL y MongoDB, y otro como `from_row(&PgRow)`; ambos se retiraron: el primero contradecía §12.1 (async) y el segundo ataba el puerto a PostgreSQL.
- [ ] `MemoryBackend` en el núcleo (§12.1, RF-015, RF-110)
- [ ] `rdd-sqlite`, `rdd-postgres` y `rdd-mysql` sobre `sqlx`, elección por esquema de URL al arrancar (RF-111, ⚠️ V11)
- [ ] `rdd-db-tests`: suite de conformidad obligatoria (RF-180 a RF-182, RA-016)
- [ ] `#[aggregate]` macro — expansión mínima por ahora (expande `item` tal cual); expansión completa planificada para 0.2.1
- [ ] `#[service]` macro — expansión mínima por ahora (expande `item` tal cual); expansión completa planificada para 0.2.1
- [ ] `Repo<T>` con los seis métodos (§12.1) sobre el puerto, verificado en memoria, PostgreSQL y MySQL
- [ ] Conversión de errores por adaptador (RF-042)
- [ ] `TestApp` (RF-160, RF-161)
- [ ] Diagnósticos RDD0101, RDD0102, RDD0201
- [ ] El scaffold del CLI usa `Repo<T>` en `repositories/`, no un `Vec` en memoria (§14.2)

**Aceptación:** `todo-app-ddd` (§25) pasa el test de §20 con backend en memoria **y** contra PostgreSQL real: crear una tarea por HTTP, verla con `psql`, reiniciar la app y comprobar que sigue ahí. `rdd-mysql` pasa `rdd-db-tests` contra un MySQL/MariaDB real.

### Fase 0.3 — Middlewares y paquetes

**Entrega:** sistema de middlewares, `rdd-cors`, `rdd-logger`, `rdd g middleware`, `rdd add`.

- [ ] Trait `Middleware`, `fn_middleware` y `AxumPlatform::layer()` para capas Tower (§11.2, RF-089)
- [ ] Registro global, de módulo, de controller y de ruta, con el orden de §11.3 verificado por tests
- [ ] Fallback cubierto por middlewares globales (RF-081)
- [ ] `Ext<T>` y `req.resolve::<T>()`
- [ ] `rdd-cors` con RF-090 a RF-099 (implementación propia, sin Tower)
- [ ] `rdd-logger` con RF-100 a RF-104
- [ ] Diagnóstico RDD0401

**Aceptación:** test que demuestra que un preflight `OPTIONS` recibe cabeceras CORS correctas sin pasar por un middleware de autenticación que rechaza todo; test de orden de ejecución con tres middlewares anidados; test de que `request_id` aparece en la respuesta y en el log.

### Fase 0.4 — Prisma

**Entrega:** `rdd-prisma`, `rdd db *`, `rdd doctor`, `rdd check`.

- [ ] Parser del subconjunto de §13.4 con errores localizados
- [ ] Generador determinista de `src/generated/` (RF-133)
- [ ] Mapeo de tipos de §13.5
- [ ] Tratamiento de las trampas de §13.8 (`@updatedAt`, ids generados por la app)
- [ ] `rdd db migrate dev|deploy|status|reset` envolviendo la CLI de Prisma fijada
- [ ] `rdd db check` (§13.7)
- [ ] `rdd check` con RA-001 a RA-006
- [ ] Verificación de las dependencias marcadas ⚠️ (§22)

**Aceptación:** partiendo de un `schema.prisma` limpio, `rdd db migrate dev --name init && rdd db generate && rdd run` deja la API funcionando contra PostgreSQL; ejecutar `rdd db generate` dos veces produce archivos idénticos byte a byte; un `schema.prisma` con una construcción no soportada falla con archivo, línea y columna.

### Fase 0.5 — Segunda plataforma

**Entrega:** `rdd-hyper`.

- [ ] `HyperPlatform` implementando `Platform`, apoyada en `HttpApp::dispatch` o en su propio enrutado
- [ ] Apagado ordenado con drenaje (RP-009), límite de cuerpo (RP-006) y normalización de barra (RP-008)
- [ ] Suite de conformidad completa (C-01 a C-12) pasando en `rdd-axum`, `rdd-hyper` y `dispatch`
- [ ] Medición de sobrecarga de la conversión de tipos y comparación entre plataformas (⚠️ V10)
- [ ] Ajuste de la interfaz `Platform` si la segunda plataforma revela que copiaba la forma de Axum (cualquier ajuste se registra como cambio de este documento)

**Aceptación:** `todo-app-ddd` arranca con `run()` (Axum) y con `run_on(HyperPlatform::new(), ...)` **sin cambiar una sola línea de código de aplicación**, y ambos pasan los mismos tests de integración.

### Fase 0.6 — Adaptadores adicionales

**Entrega:** `rdd-mongodb`, `rdd-redis`, `rdd-mssql`.

- [ ] `rdd-mongodb` con el driver oficial `mongodb` 3.x: documento completo por `_id`, `replaceOne` con upsert (⚠️ V14)
- [ ] `rdd-redis` como adaptador KV **y** como caché/sesiones/TTL en `infrastructure/cache/` (ADR-17, RF-183, RF-184, ⚠️ V13)
- [ ] `rdd-mssql` con `tiberius-ng` + `bb8` + `tokio-util` compat, `MERGE` para upsert y corchetes en identificadores (ADR-18, ⚠️ V12)
- [ ] Los tres pasan `rdd-db-tests` contra servidores reales (RA-016); SQL Server y MongoDB en Docker
- [ ] Variantes de `Value` no soportadas declaradas por adaptador (RF-182)
- [ ] `rdd-core` y los adaptadores de 0.2 siguen compilando sin cambios: si hay que tocarlos, el puerto estaba mal (RF-185)

**Aceptación:** `todo-app-ddd` arranca contra MongoDB, Redis y SQL Server cambiando solo `DATABASE_URL`, y pasa los mismos tests de integración. Ningún service ni controller cambia.

### Criterios transversales (todas las fases)

- [ ] `cargo clippy -- -D warnings`, `cargo fmt --check` y `cargo test` en CI
- [ ] La suite de conformidad (§4.5) pasa en todas las plataformas publicadas (RP-020)
- [ ] La suite de conformidad de adaptadores (§12.4) pasa en todos los motores publicados (RF-180, RA-016)
- [ ] RA-011 y RA-014 verificadas en CI (el núcleo no arrastra `axum`, `hyper` ni ningún driver de BD)
- [ ] Cobertura de pruebas de integración para cada requisito `RF-` marcado como DEBE
- [ ] Cada diagnóstico de macro tiene un test `trybuild` con su mensaje esperado
- [ ] Documentación con ejemplo ejecutable (`cargo test --doc`) por cada API pública

---

## 22. Verificaciones pendientes ⚠️

Puntos que este documento **no puede afirmar** sin comprobarlos contra las versiones reales al iniciar cada fase:

| # | Verificar | Fase | Impacto |
|---|---|---|---|
| V1 | Sintaxis de parámetros de ruta de la versión elegida de Axum (`:id` vs `{id}`). | 0.1 | Interno de `rdd-axum` (RP-003); no afecta al usuario. |
| V2 | Disponibilidad de los nombres `rdd`, `rdd-macros`, `rdd-cors`, etc. en crates.io. | 0.1 | Renombrar si están tomados. |
| V3 | Estructura exigida por la versión fijada de Prisma CLI para `datasource`/URL (`prisma.config.ts`). | 0.4 | §13.2, §15.2 |
| V4 | Que `prisma migrate` funcione sin bloque `generator` y sin cliente generado. | 0.4 | Plantilla de `rdd new`. |
| V5 | Si el parser de esquema oficial de Prisma es reutilizable como dependencia, o si se mantiene el parser propio. | 0.4 | ADR-04 |
| V6 | Crate mantenido para diagnósticos de macros (`proc-macro-error2` u otro). | 0.1 | §4.3 |
| V7 | MSRV real exigido por las dependencias elegidas. | 0.1 | §0 |
| V8 | Crate de matching de rutas para `HttpApp::dispatch` (p. ej. `matchit`) y su compatibilidad con la sintaxis `/:id`. | 0.1 | RP-014 |
| V9 | API vigente de `hyper`/`hyper-util` para servir con apagado ordenado. | 0.5 | `rdd-hyper` |
| V10 | Sobrecarga real de convertir entre los tipos del servidor y `Request`/`Response` del núcleo; decisión de si el cuerpo en memoria es suficiente. | 0.5 | §4.6, ADR-12 |
| V11 | `sqlx` 0.9 con MySQL/MariaDB: tipos que difieren de PostgreSQL (`TINYINT(1)`↔`bool`, `DATETIME`↔`NaiveDateTime`), sintaxis de upsert y entrecomillado con backticks. | 0.2 | `rdd-mysql`, RF-112/RF-114 |
| V12 | `tiberius-ng` 0.13: MSRV real (no lo publica), backend TLS a fijar, pool con `bb8`, y que la imagen `mcr.microsoft.com/mssql/server:2022-latest` arranca en el entorno de CI. | 0.6 | `rdd-mssql`, RA-016 |
| V13 | Cliente Redis a fijar (`redis-rs` async vs `fred`): MSRV, soporte de `SCAN` y de TLS, y si expone TTL por clave de forma ergonómica. | 0.6 | `rdd-redis`, RF-183/RF-184 |
| V14 | Driver oficial `mongodb` 3.x: MSRV, mapeo BSON↔`Value` (Decimal128, ObjectId, fechas) y comportamiento de `replaceOne` con `upsert`. | 0.6 | `rdd-mongodb`, RF-182 |
| V15 | `Value` no tiene variante temporal ni decimal exacta. Decidir si entran `Timestamp`/`Date`/`Decimal` (y qué crate: `chrono` + `rust_decimal` en el núcleo, o variantes opacas por motor) antes de admitir columnas `TIMESTAMPTZ`/`TIMESTAMP`/`DATE`/`NUMERIC`. Mientras tanto los adaptadores las rechazan con error explícito. | 0.4 | §12.1, RF-187, §25.5 |

---

## 23. Registro de decisiones (ADR)

| ID | Decisión | Alternativas descartadas | Motivo |
|---|---|---|---|
| ADR-01 | Un crate de aplicación, un binario. | Workspace de módulos, microservicios. | Simplicidad pedida; el diseño no lo bloquea. |
| ADR-02 | Middlewares propios del núcleo (trait `Middleware`); capas Tower solo como opción de `AxumPlatform`. *(Sustituye a la versión 0.1: «middlewares como capas Tower».)* | Middleware = capa Tower en todas partes. | Mantiene el núcleo independiente del servidor (P4) conservando acceso al ecosistema Tower en la plataforma por defecto. |
| ADR-03 | Prisma solo para esquema/migraciones; SQLx como cliente. | `prisma-client-rust`. | Proyecto sin mantenimiento; versión de Prisma antigua. |
| ADR-04 | Parsear un subconjunto de `schema.prisma`. | Introspección de BD (§13.9). | Mantiene `schema.prisma` como fuente de verdad. Revisable. |
| ADR-05 | Aggregate con `table` (simple) o `row` (separado). | Solo uno de los dos. | Simple por defecto, separación DDD real cuando se necesite. |
| ADR-06 | DI singleton validada al arrancar. | DI totalmente en compilación. | Rust no tiene reflexión; lo demás exige complejidad desproporcionada. |
| ADR-07 | Controller como struct explícito con funciones asociadas. | `impl` sin struct; métodos con `&self`. | Una macro de atributo no puede declarar el tipo; sin `self` evita estado oculto. |
| ADR-08 | Errores en RFC 9457 (Problem Details). | Formato propio. | Estándar, interoperable. |
| ADR-09 | Migraciones nunca automáticas al arrancar. | `auto_migrate`. | Evita migraciones accidentales en producción. |
| ADR-10 | ~~Solo PostgreSQL en 0.x.~~ **Sustituido por ADR-16** (0.4.0-draft). | Multi-BD desde el inicio. | Se mantuvo mientras PostgreSQL era el único motor; dejó de aplicar al pedir adaptadores para MySQL, MongoDB, Redis y MSSQL. |
| ADR-11 | Plataforma = adaptador sobre el router del servidor (modelo A). Runtime fijo: Tokio. | Motor de enrutado y transporte totalmente propios sobre hyper (modelo B); soporte multi-runtime. | Equilibrio entre portabilidad y esfuerzo; con el modelo B, Axum dejaría de aportar valor. |
| ADR-12 | Cuerpos de petición y respuesta en memoria, con límite. | Streaming desde 0.1. | Interfaz de plataforma mínima y común; el streaming se reevalúa tras la fase 0.5 (V10). |
| ADR-13 | `rdd-cors` con implementación propia. | Envolver `tower_http::cors`. | Evita acoplar el paquete a Tower; la lógica es corta y está especificada en el estándar Fetch. |
| ADR-14 | `rdd-core` + `rdd` como fachada. | Un único crate `rdd` con el núcleo y la plataforma por feature. | Evita la dependencia circular entre `rdd` y `rdd-axum`. |
| ADR-15 | La suite de conformidad es obligatoria para toda plataforma. | Confiar en pruebas por plataforma. | Sin ella, la abstracción copia la forma del único adaptador existente. |
| ADR-16 | **Persistencia multi-motor vía adaptadores**: puerto `DbAdapter` en `rdd-core`, un crate por motor, suite `rdd-db-tests` obligatoria. *(Sustituye a ADR-10.)* | (a) Solo PostgreSQL; (b) un único crate `rdd-db` con features por motor; (c) un ORM multi-motor (SeaORM/Diesel). | (a) lo pidió el usuario y el patrón ya existe para plataformas (ADR-11), así que reutilizarlo es barato. (b) arrastra todos los drivers al lockfile y mezcla cinco APIs en un crate. (c) añade una capa de abstracción ajena y Diesel es síncrono. El puerto tiene cuatro operaciones, no seis, para que portar un motor sea pequeño. |
| ADR-17 | Redis es adaptador KV **y** caché, en el mismo crate. | (a) Solo caché en `infrastructure/cache/`; (b) solo `Repo<T>`. | El usuario pidió ambos usos. Se documenta que `all()` es un `SCAN` O(N) (RF-183) para que nadie lo confunda con un `SELECT`. |
| ADR-18 | MSSQL con `tiberius-ng` + `bb8`, verificado contra SQL Server en Docker. | (a) `sqlx` (eliminó MSSQL en 0.7); (b) `tiberius` original (sin mantenimiento); (c) `odbc-api` (síncrono, exige driver ODBC en el host). | `tiberius-ng` 0.13 es la continuación mantenida, async y runtime-agnóstica. Sin servidor real no se publica (RA-016). |

---

## 24. Gestión de cambios de este documento

1. Todo cambio de diseño se propone modificando este archivo en una PR; el código se implementa **después** de aprobada la PR.
2. Cambiar o eliminar un requisito existente exige una entrada en el registro (abajo) y, si es una decisión de peso, un ADR en §23.
3. Un requisito eliminado NO reutiliza su ID.
4. La versión de este documento sigue SemVer: **mayor** = cambio incompatible de comportamiento, **menor** = requisitos nuevos, **parche** = aclaraciones.

### Registro de cambios

| Versión | Fecha | Cambio |
|---|---|---|
| 0.1.0-draft | 2026-10-29 | Versión inicial: núcleo REST, DDD mínimo, middlewares, CORS, Prisma para migraciones. |
| 0.2.0-draft | 2026-10-29 | **Cambio de arquitectura:** el núcleo deja de depender de Axum. Nueva interfaz `Platform` (§4.4), suite de conformidad (§4.5), `rdd-axum` como plataforma por defecto y `rdd-hyper` en fase 0.5. Crates `rdd-core` + fachada `rdd`. Middleware propio del núcleo (Tower solo en `AxumPlatform`); `rdd-cors` con implementación propia (RF-095 a RF-099). Extractores propios (`FromRequest`, RF-057). El núcleo abre el `TcpListener`. ADR-02 reemplazado; nuevos ADR-11 a ADR-15; nuevas verificaciones V8 a V10. |
| 0.2.1-draft | 2026-11-07 | Ajustes de implementación sobre requisitos ya existentes, verificados con `rdd-macros` en la fase 0.1. (1) **RF-057**: la firma de `FromRequest` pasa a `from_request(ctx: &Ctx, bind: Bind)`; `Ctx` es propietario de la petición y `Bind` es la pista de enlace (parámetro de ruta o cabecera), porque la firma es única para todos los extractores. (2) **§4.4**: `RouteDef::path` es `String`, no `&'static str`, porque la ruta final `prefijo + ruta` se calcula al montar (RF-051). (3) **§8.5**: la expansión de referencia se reescribe con la forma real —`rdd::api::route_def` compone middlewares, materializa errores y fija el estado; `routes()` y el registro `Controller`/`ModuleMember` quedan explícitos—, que es la que verifica RN-013. (4) **RF-052/RF-053**: `IntoResponse` es una impl blanket sobre `Serialize`, así que `Result<Task>` funciona sin envoltorios; 204 y 304 se emiten sin cuerpo. No hay `impl IntoResponse for Response<Bytes>` (sería incompatible con la blanket): el control total de la respuesta vive en los middlewares, que devuelven `Result<Response>` (§11.1). |
| 0.2.2-draft | 2026-11-07 | Cierre de la fase 0.1 con la CLI. (1) **§6.1/RF-012**: `App::run` y `App::run_on` devuelven `rdd::Result<()>` en vez de `PlatformResult`, para que `#[rdd::main] async fn main() -> Result<()>` compile tal cual; se añade `From<PlatformError> for Error` para no perder el error original. (2) **§14.1**: `rdd new` genera siempre el módulo de ejemplo `tasks` (nombre fijo del spec), no el nombre del proyecto; el nombre del proyecto solo decide el paquete y el directorio. (3) **§5 (estado temporal)**: en 0.1 el módulo de ejemplo solo lleva `mod.rs` y `routes.rs`, porque `#[aggregate]`/`#[service]` llegan en 0.2; `rdd g module` completará `domain.rs` y `service.rs`. (4) **§14.1**: `rdd run --memory` exporta `RDD_MEMORY=1` como señal forward-compatible para elegir el backend en memoria cuando exista persistencia. (5) **RF-140/RF-141**: `rdd new` crea `./<kebab-case>`, se niega a sobrescribir sin `--force` y lista sin escribir con `--dry-run`. |
| 0.3.0-draft | 2026-11-07 | **Scaffolding con la convención NestJS.** (1) **§5 se reescribe**: el proyecto pasa de `src/tasks/{mod,domain,service,routes}.rs` al árbol completo por responsabilidad —`config/`, `common/` (11 subcarpetas), `modules/<m>/` (nueve carpetas: controllers, services, repositories, dto, entities, interfaces, mappers, constants, tests), `infrastructure/`, `integrations/`, `shared/`, `tests/`— más `app_module.rs`/`app_controller.rs`/`app_service.rs`, `rustfmt.toml`, `clippy.toml`, `rust-toolchain.toml`, `Dockerfile`, `.env.example` y `.gitignore`. La adaptación de nombres (`x.y.ts`→`x_y.rs`, `__tests__`→`tests`, guiones→`snake_case`, singular de entity/dto) queda fijada en §5. (2) **RF-001 se redefine** y se añaden **RF-002 a RF-007**: árbol completo en `rdd new`, `mod.rs` en cada carpeta-módulo, cabecera documental, regla de singularización, prohibición de generar código que dependa de macros aún no implementadas, y `cargo check` + `tests/acceptance.rs` en verde. (3) **§14.2/§14.3**: el marcador `// rdd:mods` pasa a `src/modules/mod.rs` y `// rdd:modules` queda en `src/main.rs`. (4) **§17 RA-001 a RA-003**: las reglas de importación se expresan por carpeta (`entities/`, `services/`, `controllers/`) en vez de por nombre de archivo suelto. (5) **§21**: se desmarca `rdd new`/`rdd run` hasta cerrar RF-007, y se corrigen las filas de 0.2 que daban por hecho un `Persist` CRUD síncrono con MySQL/MongoDB que contradecía §12 y §4.3; ese código se retiró del árbol. (6) **§6.1/RF-012**: se confirma el retorno `rdd::Result<()>` de `App::run`/`run_on` acordado en 0.2.2, que es lo que exige `run_main` (§8.4). (7) **§5**: `rust-toolchain.toml` no fija `channel` (solo perfil y componentes); el MSRV lo declara `rust-version` en `Cargo.toml`, porque un canal pinned obligaría a descargar otro toolchain y rompería RF-143 sin red. (8) **§14.2**: se fijan los nombres que genera el CLI (`TasksController`, `TasksService`, `TasksRepository`, el puerto `TaskStore`, `Task`, `TaskMapper`, los tres DTOs y `MAX_TITLE_LEN`), el almacén aislado para tests y el mecanismo de `tests/acceptance.rs` (incluir `src/` por `#[path]`, porque un test de integración no puede importar el binario). (9) **§14.3**: el esqueleto de `main.rs` pasa a ser el que genera el CLI —`mod generated;`, configuración desde `src/config/app_config.rs`, `RDD_MEMORY` → `App::in_memory()` y el marcador seguido de `.module(app_module::module())`— y se documenta que la inserción va justo debajo del marcador, conservándolo. (10) **§21**: `rdd g module` pasa de la fase 0.2 a la 0.1, porque RF-001 lo exige y el módulo de ejemplo de `rdd new` sale del mismo generador; mantenerlos en fases distintas habría duplicado el árbol. (11) **§14.2**: el controller de ejemplo lleva los cinco verbos (`list`, `get`, `create`, `update`, `remove`), no solo los tres del boceto inicial. (12) **RF-007 se refuerza**: el proyecto generado debe pasar también `cargo clippy --all-targets -- -D warnings` y `cargo fmt --all -- --check`, y la aceptación lo comprueba tras añadir `task-items`; el scaffold entrega `clippy.toml` y `rustfmt.toml`, así que no puede nacer violándolos. |

| 0.4.0-draft | 2026-11-07 | **Persistencia multi-motor vía adaptadores.** (1) **ADR-10 queda sustituido por ADR-16**: el núcleo ya no depende de `sqlx`; define el puerto `DbAdapter` y cada motor vive en su propio crate (`rdd-postgres`, `rdd-mysql`, y en 0.6 `rdd-mongodb`, `rdd-redis`, `rdd-mssql`). Se añaden **ADR-17** (Redis es adaptador KV y API de caché en el mismo crate) y **ADR-18** (MSSQL con `tiberius-ng` + `bb8`, verificado en Docker). (2) **§2.1/§2.2**: la exclusión de motores distintos de PostgreSQL se retira; pasa a "motores sin adaptador publicado" y se acota lo que sigue fuera (SQLite barato vía `sqlx`; Oracle, Cassandra y DynamoDB no planificados). Prisma sigue siendo solo PostgreSQL (**RF-186**). (3) **§4.1/§4.2/§4.3**: entran los cinco crates de adaptador y `rdd-db-tests`; `sqlx` sale de las dependencias permitidas de `rdd-core` y entra en la lista prohibida junto con `mongodb`, `redis` y `tiberius`. Nuevas **RA-014** (el núcleo no lleva ningún driver en su árbol, verificado en CI como RA-011), **RA-015** (los adaptadores no dependen entre sí ni de ninguna plataforma) y **RA-016** (un adaptador solo se publica si pasa `rdd-db-tests` contra un servidor real). (4) **§12 se reescribe**: `Value`/`Row` neutros, puerto `DbAdapter` object-safe con cuatro operaciones (`get`, `all`, `save`, `delete`) y `Repo<T>` como handle sobre `Arc<dyn DbAdapter>`; `Persist` deja de hablar SQL y pasa a `COLUMNS` + `to_values`/`from_row`. Se rechazan las transacciones en el puerto (§12.3) porque Redis no las honra. (5) **RF-042**: la conversión de errores del driver vive en el crate del motor, nunca en el núcleo. (6) **RF-110 a RF-117**: handle agnóstico, selección por esquema de URL con fallo al arrancar nombrando el crate que falta, upsert nativo atómico, parámetros ligados, quoting por motor, handle nativo opcional, pool en `rdd.toml` y nada de migraciones automáticas. (7) **§12.4/§12.5**: suite `rdd-db-tests` (**RF-180 a RF-182**) y tabla por motor, más **RF-183** (`all()` de Redis es `SCAN`, no un `SELECT`), **RF-184** (KV y caché distinguibles, sin TTL implícito), **RF-185** (un motor nuevo no obliga a tocar el núcleo ni otros adaptadores) y **RF-186**. (8) **§14.2**: transición a 0.2 del scaffold —`TasksRepository` envuelve `Repo<Task>`, `TaskStore` pasa a ser async y `shared()`/`new()` desaparecen en favor de `MemoryBackend`/`TestApp`— sin cambiar el árbol de carpetas. (9) **§15.1**: `[database] provider` pasa a ser informativo para `rdd doctor` y admite los seis valores; la URL sigue viniendo solo del entorno (RF-063). (10) **§17**: RA-001/RA-003 amplían la prohibición a cualquier driver y se añade **RA-007** (`repositories/` es la única carpeta que toca un adaptador). (11) **§21**: la fase 0.2 incorpora el puerto, `MemoryBackend`, `rdd-db-tests`, `rdd-postgres` y `rdd-mysql`, con aceptación contra PostgreSQL real (crear por HTTP, verlo con `psql`, reiniciar y comprobar que sobrevive); se crea la **fase 0.6** para los adaptadores adicionales. (12) **§22**: nuevas verificaciones **V11** (diferencias MySQL en `sqlx`), **V12** (`tiberius-ng`), **V13** (cliente Redis) y **V14** (BSON↔`Value`). |

| 0.4.1-draft | 2026-11-07 | Detalles de implementación del puerto, necesarios antes de escribir `rdd-core`. (1) **§12.1**: `Repo<T>` añade la cota `T::Id: Into<Value>` —`Aggregate::Id` es un tipo asociado libre y el puerto solo habla `Value`—, se fija `Repo::new(Arc<dyn DbAdapter>)` y se documenta que `Repo<T>` implementa `FromContainer` leyendo el valor `Db` que `App::build()` registra, de modo que `#[service]` lo inyecta sin registro por agregado. (2) **§12.6 nuevo**: registro global de adaptadores (`declare`/`install`/`factory_for`) poblado por la fachada, con el mismo razonamiento que RP-012 para la plataforma; es lo que permite nombrar el crate que falta sin que el núcleo conozca ningún motor (RF-185). (3) **RF-118/RF-119 nuevos**: `App::build()` sigue siendo síncrono y sin I/O, y `App::connect().await` resuelve la URL y conecta; `.run()`/`.run_on()` lo llaman antes de componer, así que el fallo de RF-111 ocurre antes de abrir el socket. `.adapter(Arc<dyn DbAdapter>)` inyecta uno ya conectado. Se mantiene `build()` síncrono para no volver async toda la suite de plataformas ni el scaffold. (4) **RF-015**: `.in_memory()` tiene prioridad sobre una URL ya configurada. (5) **§12.1**: las cuatro operaciones de `DbAdapter` llevan una vida explícita común `'a` en todos sus argumentos; con `BoxFuture<'_>` la elisión la ata solo a `&self` y el `async move` del adaptador no puede capturar `table` ni `id`. El trait sigue siendo object-safe. |

| 0.4.2-draft | 2026-11-07 | **App de ejemplo en la raíz: `todo-app-ddd`.** Petición del usuario. (1) **§4.1**: desaparece `examples/todo-app/` y entra `todo-app-ddd/` en la raíz del repositorio, miembro del workspace. La razón es que así `cargo test --workspace` la compila y un cambio en el núcleo que rompa el scaffold se detecta en la misma corrida. (2) **§25 se reescribe entero**: el anexo mostraba un árbol plano `src/tasks/{domain,service,routes,mod}.rs` que contradecía §5 desde 0.3.0-draft, y usaba `Uuid::new()` y macros aún no implementadas. Ahora describe la app real: árbol de §5, `Task` con `Aggregate`+`Persist` escritos a mano hasta que llegue `#[aggregate]` (RF-006), `TasksRepository` envolviendo `Repo<Task>`, `TaskStore` con `async fn` nativo (sin `async_trait`, como `FromRequest`), arranque con `.database(url)` y `.env` no versionado, tabla creada con `migrations/0001_tasks.sql` hasta que `rdd db migrate` exista en 0.4, y la verificación completa contra PostgreSQL real. (3) **§5, §15.1 y §21**: las tres fases que usaban el ejemplo (0.2, 0.5 y 0.6) pasan a nombrar `todo-app-ddd`. |

| 0.4.3-draft | 2026-11-07 | **El puerto gana `id_column`**, descubierto al implementar el segundo adaptador — exactamente el fallo que ADR-15 y `rdd-db-tests` existen para detectar. (1) **§12.1**: `get` y `delete` reciben `id_column: &str` además de la tabla y el id. Un motor SQL necesita el nombre de la columna para el `WHERE` y no puede adivinarlo, porque `Persist::COLUMNS[0]` puede llamarse `task_id` y no `id`; inferirlo de `save` obligaría a guardar antes de poder leer. `Repo<T>` lo pasa desde `T::COLUMNS[0]`; MongoDB y Redis lo ignoran. (2) **§12.1**: el orden de `Row` deja de ser contractual. `all` no recibe la lista de columnas y hace `SELECT *`, así que la fila llega en el orden físico de la tabla; `from_row` lee por nombre y las columnas de más se ignoran. Afirmar el orden de `COLUMNS` habría sido falso para cualquier motor SQL. |

| 0.4.4-draft | 2026-11-07 | **Tipos que `Value` no puede representar**, detectado al escribir el códec de `rdd-postgres`. (1) **RF-187 nuevo**: un adaptador solo decodifica tipos con variante simétrica en `Value`; los demás dan error explícito nombrando columna y tipo. Convertir `TIMESTAMPTZ` a `String` sería una trampa: se lee bien pero al guardar se envía el OID de `TEXT` y PostgreSQL lo rechaza. (2) **§12.5**: la fila de PostgreSQL remite a RF-187. (3) **V15 nueva**: falta decidir si `Value` gana variantes temporal y decimal antes de la fase 0.4. (4) **§25.5**: la tabla `tasks` pierde `created_at TIMESTAMPTZ`, que era incompatible con lo anterior. (5) **§4.2**: `anyhow` entra en las dependencias permitidas de los cinco adaptadores —`Error::internal` lo exige para conservar el error del driver como fuente (RF-042)— y se corrige la dirección de la suite: cada adaptador depende de `rdd-db-tests` como `dev-dependency`, no al revés. (6) **§15.2**: la plantilla de `schema.prisma` pierde `createdAt DateTime` por la misma razón que §25.5 — el código generado desde ese esquema no podría leer la columna hasta que V15 se resuelva. (7) **§12.4**: se fija el mecanismo de RF-181 — el test de integración de cada motor se activa con `RDD_TEST_<MOTOR>_URL`, y el crate crea y destruye su propia tabla de sonda. (8) **§25.4/§25.6**: la instancia local pasa al contenedor `rdd-pg` en el puerto **5433**. En 5432 el servicio nativo de Windows le gana la conexión al proxy de Docker, así que el contenedor es inalcanzable desde el host; ni el servicio nativo ni `todo-pg` se modifican. |
| 0.4.5-draft | 2026-11-07 | Se añade `rdd-sqlite` como adaptador predeterminado para proyectos locales y se actualizan la fachada, el scaffold y las guías. |
| 0.4.6-draft | 2026-11-07 | Se añade `rdd migrate` para aplicar migraciones versionadas de SQLite/PostgreSQL como paso explícito de despliegue (RF-188); el servidor no migra al arrancar. Se fija Rust 1.94 como MSRV por SQLx 0.9. |

---

## 25. Anexo: la app de ejemplo `todo-app-ddd`

### 25.1 Qué es y dónde vive

`todo-app-ddd/` está en la **raíz del repositorio**, es miembro del workspace y
**la genera el CLI** (`rdd new todo-app-ddd`): no se mantiene a mano. Es la
aceptación viva de la fase 0.2 (§21), un CRUD de tareas contra PostgreSQL real.

Vive en la raíz y no en `examples/` porque es el banco de pruebas del framework:
al ser miembro del workspace, `cargo test --workspace` la compila y la ejercita, así
que un cambio en el núcleo que rompa el scaffold se detecta en la misma corrida que
los tests del núcleo. Sustituye al antiguo `examples/todo-app/` de §4.1.

### 25.2 Estructura

El árbol completo de §5, sin excepciones: `config/`, `common/`, `modules/tasks/`
con sus nueve carpetas, `infrastructure/`, `integrations/`, `shared/` y `tests/`.
Los nombres son los de §14.2 (`Task`, `TasksController`, `TasksService`,
`TasksRepository`, el puerto `TaskStore`, `TaskMapper`, los tres DTOs y
`MAX_TITLE_LEN`). Este anexo no repite el árbol: §5 es la referencia.

### 25.3 Persistencia

`Task` implementa `Aggregate` (§7.1) y `Persist` (§12.2) con `TABLE = "tasks"` y
`COLUMNS = ["id", "title", "done", "created_at"]`. Mientras `#[aggregate]` no esté
implementada (RF-006), esas dos impls se escriben a mano en
`modules/tasks/entities/task_entity.rs`; la macro las sustituirá sin cambiar el
resto del módulo.

`TasksRepository` envuelve `Repo<Task>` y implementa el puerto `TaskStore`, que es
**async** (§14.2, transición a 0.2). El service depende de `TaskStore`, nunca del
crate del motor (RA-007): cambiar `DATABASE_URL` a otro motor no toca ni el service
ni el controller (RF-185).

```rust
// modules/tasks/repositories/tasks_repository.rs
pub struct TasksRepository { repo: Repo<Task> }

impl TasksRepository {
    pub fn new(repo: Repo<Task>) -> Self { Self { repo } }
}

impl TaskStore for TasksRepository {
    async fn list(&self) -> Result<Vec<Task>> { self.repo.all().await }
    async fn get(&self, id: Uuid) -> Result<Task> { self.repo.get(id).await }
    async fn save(&self, task: &Task) -> Result<()> { self.repo.save(task).await }
    async fn remove(&self, id: Uuid) -> Result<()> { self.repo.delete(id).await }
}
```

`TaskStore` usa `async fn` nativo en el trait, como `FromRequest` (§6.3): el service
es genérico sobre el puerto, así que no necesita object-safety ni el crate
`async_trait`.

No hay `shared()` ni `new()` con un `static` global: el aislamiento de los tests lo
dan `MemoryBackend` y `TestApp` (RF-160), y el `Repo<Task>` se inyecta desde el
contenedor (§12.1).

### 25.4 Arranque

El `main.rs` es el de §14.3. La URL viene **solo** del entorno (RF-063), en un
`.env` no versionado:

```bash
# todo-app-ddd/.env — NO se commitea
DATABASE_URL=postgres://postgres:<clave>@localhost:5433/todo_db
```

**Puerto 5433, no 5432.** En la máquina de desarrollo hay dos PostgreSQL: el
servicio nativo de Windows (`postgresql-x64-17`) y Docker. Los dos publican 5432 y
el nativo gana la conexión, así que un contenedor en 5432 es inalcanzable desde el
host — el síntoma es un `FATAL` de autenticación con mensajes en un codepage que no
es UTF-8, que sqlx reporta como «non-UTF-8 string for its error message». La
instancia de pruebas es el contenedor `rdd-pg` (`postgres:15-alpine`, BD `todo_db`)
publicado en **5433**; ni el servicio nativo ni `todo-pg` se tocan.

`App::new().database(url)` no conecta: `.run()` llama a `connect().await`, que
resuelve el esquema `postgres://` por el registro de §12.6 y conecta el pool antes
de abrir el socket (RF-111, RF-119). Sin `.env` o con `RDD_MEMORY=1`, la app arranca
en memoria (RF-015).

### 25.5 Esquema de la tabla

El ejemplo crea su tabla con SQL plano en
`todo-app-ddd/migrations/0001_tasks.sql`. Se aplica con `rdd migrate`, que
mantiene el historial y es reutilizable en CI/CD; Prisma podrá generar estos
archivos desde `schema.prisma` cuando entre.

```sql
CREATE TABLE IF NOT EXISTS tasks (
    id    UUID PRIMARY KEY,
    title TEXT    NOT NULL,
    done  BOOLEAN NOT NULL DEFAULT FALSE
);
```

Sin `created_at`: `Value` no tiene variante temporal (**V15**) y una columna
`TIMESTAMPTZ` rompería el round-trip — el adaptador la rechazaría al leer
(RF-187) y ligar un `String` enviaría el OID de `TEXT`, que la columna no acepta.
La columna entra cuando V15 se resuelva, no antes.

### 25.6 Verificación (aceptación de la fase 0.2)

```bash
docker start rdd-pg                                    # PostgreSQL local, puerto 5433
rdd migrate                                             # paso explícito antes de desplegar/arrancar
cargo run -p todo-app-ddd                              # o: rdd run

curl -X POST localhost:3000/tasks \
     -H 'content-type: application/json' -d '{"title":"Comprar leche"}'
curl localhost:3000/tasks                              # la tarea aparece
psql "$DATABASE_URL" -c 'SELECT id, title, done FROM tasks;'   # está en la BD

# reiniciar la app y repetir el GET: la tarea SIGUE ahí (eso es la aceptación)
curl -X PUT localhost:3000/tasks/<id> -H 'content-type: application/json' -d '{"done":true}'
curl -X DELETE localhost:3000/tasks/<id>
curl -i localhost:3000/tasks/<id>                      # 404 Problem Details
```

### 25.7 Flujo de trabajo del desarrollador

```bash
rdd new todo-app-ddd && cd todo-app-ddd
rdd g module task-items              # nuevo módulo con el árbol de §5
rdd migrate                          # antes del primer arranque y de cada despliegue
rdd run                              # servidor (RDD_MEMORY=1 para memoria)
rdd check                            # reglas de arquitectura (CI)
```

### 25.8 Cambiar de plataforma (sin tocar el código de aplicación)

```toml
# Cargo.toml — Axum (por defecto)
rdd = "..."

# Cargo.toml — plataforma propia
# rdd = { version = "...", default-features = false, features = ["hyper"] }
```

```rust
// main.rs
App::new()
    .database(env("DATABASE_URL"))
    .middleware(Cors::from_config())
    .middleware(Logger::default())
    .module(tasks::module())
    // Axum:   .run("0.0.0.0:3000")
    // Hyper:  .run_on(HyperPlatform::new(), "0.0.0.0:3000")
    // Axum con una capa Tower propia de la plataforma:
    .run_on(AxumPlatform::new().layer(CompressionLayer::new()), "0.0.0.0:3000")
    .await
```
