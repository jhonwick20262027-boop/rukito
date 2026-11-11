-- Cuentas del demo (§26.5). El CLI generó `0001_tasks.sql` (RF-192); esta la
-- añade el demo. `rdd migrate` lleva la cuenta en `_sqlx_migrations` y no la
-- reaplica (§14.1), así que el `ALTER TABLE` es seguro sobre una base vacía.
--
-- Los tipos declarados son los que esperan los adaptadores: SQLite guarda el id
-- como texto pero decodifica por el nombre del tipo, así que se declara `UUID`
-- igual que en PostgreSQL.
CREATE TABLE IF NOT EXISTS users (
    id            UUID PRIMARY KEY NOT NULL,
    email         TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    role          TEXT NOT NULL DEFAULT 'user'
);

-- Quién es dueño de cada tarea. Sin `NOT NULL`: la columna se añade a una tabla
-- que ya existe y el demo parte de cero filas, así que no hay valor que rellenar.
ALTER TABLE tasks ADD COLUMN owner_id UUID;

CREATE INDEX IF NOT EXISTS tasks_owner_id_idx ON tasks (owner_id);