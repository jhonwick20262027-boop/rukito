//! Ruta, explorer y profundidad de `/graphql` (§10.9, RF-242).
//!
//! Los valores por defecto son los que usaba `main.rs` a mano: `/graphql`,
//! explorer activo y público (la UI se sirve sin sesión; las operaciones siguen
//! exigiendo el JWT) y profundidad 16. En producción conviene
//! `GRAPHQL_EXPLORER=false` o, al menos, `GRAPHQL_PUBLIC_EXPLORER=false`.

pub struct GraphqlConfig {
    pub path: String,
    pub explorer: bool,
    pub public_explorer: bool,
    pub max_depth: usize,
}

pub fn load() -> GraphqlConfig {
    GraphqlConfig {
        path: super::env_or("GRAPHQL_PATH", "/graphql"),
        explorer: super::env_or("GRAPHQL_EXPLORER", "true") == "true",
        public_explorer: super::env_or("GRAPHQL_PUBLIC_EXPLORER", "true") == "true",
        max_depth: super::env_or("GRAPHQL_MAX_DEPTH", "16")
            .parse()
            .unwrap_or(16),
    }
}
