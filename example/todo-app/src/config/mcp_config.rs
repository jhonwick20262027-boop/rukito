//! `MCP_ALLOWED_ORIGINS`: orígenes de navegador que pueden invocar `/mcp` (§26.8).
//!
//! Sin variable no se permite ninguno, así que los clientes no navegador (los que
//! no mandan `Origin`) siguen entrando (RF-228). La lista es propia y no se toma
//! de `[cors]`: un origen permitido para el REST no tiene por qué poder invocar
//! las tools.

/// Lista separada por comas; los espacios alrededor de cada origen se ignoran.
pub fn allowed_origins() -> Vec<String> {
    super::env_opt("MCP_ALLOWED_ORIGINS")
        .map(|raw| {
            raw.split(',')
                .map(str::trim)
                .filter(|origin| !origin.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}
