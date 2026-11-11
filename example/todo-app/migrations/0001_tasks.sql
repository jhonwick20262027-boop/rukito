-- Migración inicial del módulo de ejemplo (RF-192).
-- La aplica `rdd migrate` (RF-188); el servidor no migra al arrancar (RF-117).
-- Los tipos declarados son los que esperan los adaptadores: SQLite guarda el id
-- como texto y el booleano como 0/1, pero decodifica por el nombre del tipo.
CREATE TABLE IF NOT EXISTS tasks (
    id    UUID PRIMARY KEY NOT NULL,
    title TEXT NOT NULL,
    done  BOOLEAN NOT NULL DEFAULT FALSE
);
