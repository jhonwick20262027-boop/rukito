//! Escritura compartida por `rdd new` y `rdd g module` (RF-140, RF-141, §14.3).
//!
//! Los comandos describen un plan (archivos nuevos + inserciones en marcadores)
//! y lo pasan por aquí, de modo que `--dry-run` y `--force` se comporten igual en
//! todos ellos.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

/// Un archivo nuevo que el comando quiere escribir.
pub struct GeneratedFile {
    pub path: PathBuf,
    pub contents: String,
}

impl GeneratedFile {
    pub fn new(path: impl Into<PathBuf>, contents: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            contents: contents.into(),
        }
    }
}

/// Una línea que se inserta después de un marcador de comentario (§14.3).
pub struct Insertion {
    pub path: PathBuf,
    pub marker: &'static str,
    pub line: String,
}

impl Insertion {
    pub fn new(path: impl Into<PathBuf>, marker: &'static str, line: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            marker,
            line: line.into(),
        }
    }
}

/// Lista o aplica el plan sobre `root`.
///
/// Con `dry_run` no toca el disco (RF-141). Sin `force` se niega a pisar archivos
/// que ya existen (RF-140); las inserciones no cuentan como sobrescritura porque
/// §14.3 las pide sobre archivos existentes. Al aplicar, los `.rs` escritos salen
/// formateados (RF-007).
pub fn apply(
    root: &Path,
    files: &[GeneratedFile],
    insertions: &[Insertion],
    force: bool,
    dry_run: bool,
) -> Result<()> {
    let conflicts: Vec<PathBuf> = files
        .iter()
        .map(|file| root.join(&file.path))
        .filter(|path| path.exists())
        .collect();

    if dry_run {
        for file in files {
            let path = root.join(&file.path);
            let verb = if path.exists() {
                "sobrescribiría"
            } else {
                "crearía"
            };
            println!("{verb} {}", path.display());
        }
        for insertion in insertions {
            println!("modificaría {}", root.join(&insertion.path).display());
        }
        if !conflicts.is_empty() && !force {
            println!(
                "aviso: {} archivo(s) ya existen; sin --force el comando se negaría (RF-140)",
                conflicts.len()
            );
        }
        return Ok(());
    }

    if !conflicts.is_empty() && !force {
        let list = conflicts
            .iter()
            .map(|path| format!("  - {}", path.display()))
            .collect::<Vec<_>>()
            .join("\n");
        bail!(
            "ya existen archivos del proyecto (RF-140):\n{list}\n\
             usa `--force` para sobrescribirlos o `--dry-run` para verlos"
        );
    }

    for file in files {
        let path = root.join(&file.path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("no se pudo crear {}", parent.display()))?;
        }
        fs::write(&path, &file.contents)
            .with_context(|| format!("no se pudo escribir {}", path.display()))?;
    }

    for insertion in insertions {
        insert(root, insertion)?;
    }
    format_rust(root, files)?;
    Ok(())
}

/// Formatea los `.rs` que se acaban de escribir (RF-007, §5.2).
///
/// El ancho de las firmas generadas crece con el nombre del módulo, así que
/// ninguna plantilla fija pasa `cargo fmt --check` con `tasks` y con `task-items`
/// a la vez. Se llama al binario y no a `cargo fmt` para no consultar el registro
/// de crates, que RF-143 exige poder evitar sin red.
fn format_rust(root: &Path, files: &[GeneratedFile]) -> Result<()> {
    // Las rutas van relativas a `root`, que es el directorio del proceso: unirlas
    // aquí duplicaría el prefijo cuando `rdd new` pasa una raíz relativa.
    let paths: Vec<&Path> = files
        .iter()
        .map(|file| file.path.as_path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "rs"))
        .collect();
    if paths.is_empty() {
        return Ok(());
    }

    let output = Command::new("rustfmt")
        .current_dir(root)
        .args(["--edition", "2024", "--config", "skip_children=true"])
        .args(&paths)
        .output();
    let output = match output {
        Ok(output) => output,
        Err(err) => {
            println!(
                "aviso: no se pudo ejecutar rustfmt ({err}); los archivos quedaron escritos \
                 sin formatear. Instálalo con `rustup component add rustfmt` y corre \
                 `cargo fmt --all` (RF-007)."
            );
            return Ok(());
        }
    };

    if !output.status.success() {
        bail!(
            "rustfmt rechazó el código generado (RF-007):\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

/// Inserta la línea después del marcador, conservando el marcador.
///
/// Si el archivo o el marcador no existen, imprime lo que hay que pegar a mano en
/// vez de fallar (§14.3). Si la línea ya está, no la duplica.
fn insert(root: &Path, insertion: &Insertion) -> Result<()> {
    let path = root.join(&insertion.path);
    let manual = |reason: &str| {
        println!(
            "aviso: {reason} {}; pega a mano:\n  {}",
            path.display(),
            insertion.line
        );
    };

    let original = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(_) => {
            manual("no se pudo leer");
            return Ok(());
        }
    };

    let wanted = insertion.line.trim();
    if original.lines().any(|line| line.trim() == wanted) {
        return Ok(());
    }

    let mut lines: Vec<String> = original.split_inclusive('\n').map(str::to_owned).collect();
    let Some(index) = lines
        .iter()
        .position(|line| line.trim() == insertion.marker)
    else {
        manual(&format!("no tiene el marcador `{}`;", insertion.marker));
        return Ok(());
    };

    // El marcador puede ser la última línea y no traer salto; sin esto la línea
    // nueva quedaría pegada al comentario.
    if !lines[index].ends_with('\n') {
        lines[index].push('\n');
    }
    let ending = if original.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    // La comparación ignora la indentación, pero la línea se inserta tal cual:
    // en `main.rs` el `.module(…)` va indentado dentro de la cadena de builders.
    let entry = insertion.line.trim_end_matches(['\r', '\n']);
    lines.insert(index + 1, format!("{entry}{ending}"));

    fs::write(&path, lines.concat())
        .with_context(|| format!("no se pudo escribir {}", path.display()))?;
    println!("registrado en {}", path.display());
    Ok(())
}

/// Sube desde el directorio actual hasta encontrar un `Cargo.toml`.
///
/// Lo comparten `rdd run` y `rdd g module`: los dos operan sobre el proyecto en
/// el que se está, no sobre uno nuevo.
pub fn project_root() -> Result<PathBuf> {
    let mut current = std::env::current_dir().context("no se pudo leer el directorio actual")?;
    loop {
        if current.join("Cargo.toml").is_file() {
            return Ok(current);
        }
        if !current.pop() {
            bail!("no se encontró `Cargo.toml` en el directorio actual ni en sus padres");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rdd-files-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn inserts_after_the_marker_and_keeps_it() {
        let dir = scratch("marker");
        fs::write(dir.join("mod.rs"), "// rdd:mods\npub mod a;\n").unwrap();
        insert(&dir, &Insertion::new("mod.rs", "// rdd:mods", "pub mod b;")).unwrap();
        assert_eq!(
            fs::read_to_string(dir.join("mod.rs")).unwrap(),
            "// rdd:mods\npub mod b;\npub mod a;\n"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn does_not_duplicate_an_existing_line() {
        let dir = scratch("duplicate");
        fs::write(dir.join("mod.rs"), "// rdd:mods\npub mod a;\n").unwrap();
        let insertion = Insertion::new("mod.rs", "// rdd:mods", "pub mod a;");
        insert(&dir, &insertion).unwrap();
        insert(&dir, &insertion).unwrap();
        assert_eq!(
            fs::read_to_string(dir.join("mod.rs")).unwrap(),
            "// rdd:mods\npub mod a;\n"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn marker_at_end_of_file_without_newline() {
        let dir = scratch("eof");
        fs::write(dir.join("main.rs"), "App::new()\n    // rdd:modules").unwrap();
        insert(
            &dir,
            &Insertion::new("main.rs", "// rdd:modules", "    .module(m::module())"),
        )
        .unwrap();
        assert_eq!(
            fs::read_to_string(dir.join("main.rs")).unwrap(),
            "App::new()\n    // rdd:modules\n    .module(m::module())\n"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_marker_does_not_fail() {
        let dir = scratch("no-marker");
        fs::write(dir.join("main.rs"), "fn main() {}\n").unwrap();
        insert(
            &dir,
            &Insertion::new("main.rs", "// rdd:modules", ".module(m::module())"),
        )
        .unwrap();
        assert_eq!(
            fs::read_to_string(dir.join("main.rs")).unwrap(),
            "fn main() {}\n"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// RF-007: el scaffold nace formateado. La plantilla no puede estarlo para
    /// cualquier nombre de módulo, así que lo garantiza el escritor.
    #[test]
    fn formats_the_rust_files_it_writes_and_leaves_the_rest_alone() {
        let dir = scratch("format");
        let files = [
            GeneratedFile::new("src/main.rs", "fn  main( ){let x=1;println!(\"{x}\");}\n"),
            GeneratedFile::new("rdd.toml", "provider  =  \"sqlite\"\n"),
        ];
        apply(&dir, &files, &[], false, false).unwrap();

        assert_eq!(
            fs::read_to_string(dir.join("src/main.rs")).unwrap(),
            "fn main() {\n    let x = 1;\n    println!(\"{x}\");\n}\n"
        );
        assert_eq!(
            fs::read_to_string(dir.join("rdd.toml")).unwrap(),
            "provider  =  \"sqlite\"\n"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn dry_run_does_not_format_or_write() {
        let dir = scratch("format-dry");
        let files = [GeneratedFile::new("main.rs", "fn  main( ){}\n")];
        apply(&dir, &files, &[], false, true).unwrap();
        assert!(!dir.join("main.rs").exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
