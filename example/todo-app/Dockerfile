# Imagen de la app. Requiere `rdd` publicado en crates.io; mientras tanto,
# cambia la dependencia de Cargo.toml a una ruta o a un git.

FROM rust:1 AS build
WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=build /app/target/release/todo-app /usr/local/bin/app
ENV HOST=0.0.0.0 PORT=3000
EXPOSE 3000
CMD ["app"]
