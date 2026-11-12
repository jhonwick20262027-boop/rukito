//! Decodificación y validación de la ruta pedida (§10.11).
//!
//! La ruta se decodifica **antes** de dividir en segmentos: `%2e%2e%2f` cuenta
//! como `../`, no como un nombre de archivo. Ningún segmento puede salir de la
//! raíz, aunque el sistema de archivos no normalice igual que nosotros.

/// Decodifica `%XX` y rechaza bytes NUL o secuencias inválidas.
pub(crate) fn decode(input: &str) -> Option<String> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = hex(bytes.get(index + 1).copied()?)?;
            let low = hex(bytes.get(index + 2).copied()?)?;
            let byte = high * 16 + low;
            if byte == 0 {
                return None;
            }
            out.push(byte);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(out).ok()
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// Quita el prefijo del montaje; `None` si la ruta no cuelga de él.
pub(crate) fn relative<'a>(path: &'a str, mount: &str) -> Option<&'a str> {
    if mount == "/" {
        return path.strip_prefix('/');
    }
    let rest = path.strip_prefix(mount)?;
    if rest.is_empty() {
        return Some("");
    }
    rest.strip_prefix('/')
}

/// Segmentos seguros bajo la raíz; `None` si alguno intenta escapar.
///
/// Rechaza `..`, `.`, segmentos vacíos, separadores de Windows, `:` (unidad o
/// flujo alternativo), caracteres reservados y controles. Los archivos ocultos
/// solo pasan si `dotfiles` está activo.
pub(crate) fn segments(relative: &str, dotfiles: bool) -> Option<Vec<String>> {
    if relative.is_empty() {
        return Some(Vec::new());
    }
    let mut out = Vec::new();
    for raw in relative.split('/') {
        if raw.is_empty() || raw == "." || raw == ".." {
            return None;
        }
        if raw.chars().any(|ch| {
            ch.is_control() || matches!(ch, '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
        }) {
            return None;
        }
        if raw.starts_with('.') && !dotfiles {
            return None;
        }
        out.push(raw.to_owned());
    }
    Some(out)
}

/// `true` si la ruta pedida apunta a un archivo con extensión.
pub(crate) fn looks_like_file(relative: &str) -> bool {
    let last = relative.rsplit('/').next().unwrap_or(relative);
    last.len() > 1 && !last.starts_with('.') && last.contains('.')
}

/// Content-Type con `charset=utf-8` cuando el tipo es textual.
pub(crate) fn content_type(path: &std::path::Path) -> String {
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    let essence = mime.essence_str();
    let textual = essence.starts_with("text/")
        || essence.ends_with("+json")
        || essence.ends_with("+xml")
        || matches!(
            essence,
            "application/json"
                | "application/javascript"
                | "application/xml"
                | "application/xhtml+xml"
                | "image/svg+xml"
        );
    if textual {
        format!("{essence}; charset=utf-8")
    } else {
        essence.to_owned()
    }
}

/// ETag fuerte a partir de tamaño y fecha de modificación.
pub(crate) fn etag(metadata: &std::fs::Metadata) -> String {
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    format!("\"{:x}-{:x}\"", metadata.len(), modified)
}

/// `If-None-Match` coincide con el ETag actual (acepta la forma débil `W/`).
pub(crate) fn not_modified(header: Option<&str>, etag: &str) -> bool {
    let Some(header) = header else {
        return false;
    };
    header.split(',').any(|candidate| {
        let candidate = candidate.trim();
        candidate == "*" || candidate == etag || candidate.strip_prefix("W/") == Some(etag)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodifica_y_rechaza_escapes_invalidos() {
        assert_eq!(decode("/a%20b").as_deref(), Some("/a b"));
        assert_eq!(decode("/%2e%2e/secreto").as_deref(), Some("/../secreto"));
        assert_eq!(decode("/%00"), None);
        assert_eq!(decode("/%zz"), None);
        assert_eq!(decode("/%2"), None);
    }

    #[test]
    fn los_segmentos_no_escapan_de_la_raiz() {
        assert_eq!(segments("assets/app.js", false).map(|s| s.len()), Some(2));
        assert_eq!(segments("", false), Some(Vec::new()));
        assert_eq!(segments("../secreto", false), None);
        assert_eq!(segments("a/../../b", false), None);
        assert_eq!(segments("a//b", false), None);
        assert_eq!(segments("a\\b", false), None);
        assert_eq!(segments("C:/Windows", false), None);
        assert_eq!(segments(".env", false), None);
        assert!(segments(".env", true).is_some());
    }

    #[test]
    fn montaje_recorta_solo_su_prefijo() {
        assert_eq!(
            relative("/site/assets/app.js", "/site"),
            Some("assets/app.js")
        );
        assert_eq!(relative("/site", "/site"), Some(""));
        assert_eq!(relative("/site/", "/site"), Some(""));
        assert_eq!(relative("/sitio/app.js", "/site"), None);
        assert_eq!(relative("/a/b", "/"), Some("a/b"));
    }

    #[test]
    fn etag_y_304() {
        assert!(not_modified(Some("\"abc\""), "\"abc\""));
        assert!(not_modified(Some("W/\"abc\""), "\"abc\""));
        assert!(not_modified(Some("*"), "\"abc\""));
        assert!(not_modified(Some("\"otro\", \"abc\""), "\"abc\""));
        assert!(!not_modified(Some("\"otro\""), "\"abc\""));
        assert!(!not_modified(None, "\"abc\""));
    }
}
