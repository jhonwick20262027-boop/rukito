//! Aceptación RF-143 y RF-007: un proyecto generado compila y pasa sus propios
//! tests sin cambios manuales, y `rdd g module` sobre él también.
//!
//! Es lento (compila el framework y sus dependencias), así que está marcado
//! `#[ignore]`. En CI se ejecuta con:
//!
//! ```text
//! cargo test -p rdd-cli --test acceptance -- --ignored
//! ```

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn workspace() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// `cargo` dentro del proyecto generado, con el `target` y el wrapper del
/// workspace para no recompilar el framework desde cero en cada corrida.
fn cargo(dir: &Path, args: &[&str]) -> Output {
    let root = workspace();
    let mut command = Command::new("cargo");
    command.current_dir(dir).args(args).env(
        "CARGO_TARGET_DIR",
        std::env::temp_dir().join("rdd-cli-acceptance-target"),
    );
    let wrapper = root.join("scripts/rustc-gnu.exe");
    if wrapper.exists() {
        command.env("RUSTC_WRAPPER", wrapper);
    }
    command.output().expect("`cargo` debe ejecutarse")
}

fn assert_ok(output: &Output, what: &str) {
    assert!(
        output.status.success(),
        "{what} falló:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

fn generate(dir: &Path, name: &str, extra: &[&str]) {
    let output = Command::new(env!("CARGO_BIN_EXE_rdd"))
        .current_dir(dir)
        .arg("new")
        .arg(name)
        .arg("--rdd-path")
        .arg(workspace().join("crates/rdd"))
        .args(extra)
        .output()
        .expect("la CLI debe ejecutarse");
    assert_ok(&output, "`rdd new`");
}

/// RF-007: el scaffold nace limpio, no solo compilando. Entrega `clippy.toml` y
/// `rustfmt.toml`, así que violarlos sería contradictorio.
fn assert_clean(project: &Path) {
    assert_ok(
        &cargo(
            project,
            &["clippy", "--all-targets", "--", "-D", "warnings"],
        ),
        "RF-007: `cargo clippy -D warnings`",
    );
    assert_ok(
        &cargo(project, &["fmt", "--all", "--", "--check"]),
        "RF-007: `cargo fmt --check`",
    );
}

#[test]
#[ignore = "compila proyectos completos; se corre en CI con --ignored"]
fn generated_projects_compile_and_pass_their_tests() {
    let temp = std::env::temp_dir().join(format!("rdd-cli-acceptance-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp);
    std::fs::create_dir_all(&temp).expect("se debe poder crear el directorio temporal");

    // Con el módulo de ejemplo: `cargo check` (RF-143) y `cargo test` (RF-007,
    // que exige que tests/acceptance.rs del proyecto pase).
    generate(&temp, "todo-app", &[]);
    let project = temp.join("todo-app");
    assert_ok(&cargo(&project, &["check"]), "RF-143: `cargo check`");
    assert_ok(&cargo(&project, &["test"]), "RF-007: `cargo test`");
    assert_clean(&project);

    // `rdd g module` sobre el proyecto ya generado tiene que seguir compilando.
    // El nombre es de varias palabras a propósito: ejercita la singularización
    // (RF-005) y las líneas que crecen con el nombre del módulo.
    let output = Command::new(env!("CARGO_BIN_EXE_rdd"))
        .current_dir(&project)
        .args(["g", "module", "task-items"])
        .output()
        .expect("la CLI debe ejecutarse");
    assert_ok(&output, "`rdd g module`");
    assert!(
        project
            .join("src/modules/task_items/dto/create_task_item_dto.rs")
            .is_file()
    );
    assert_ok(&cargo(&project, &["test"]), "RF-007 tras `rdd g module`");
    assert_clean(&project);

    // Sin el ejemplo, el árbol tiene que compilar igual (RF-002).
    generate(&temp, "bare-app", &["--no-example"]);
    let bare = temp.join("bare-app");
    assert_ok(&cargo(&bare, &["test"]), "RF-007 con --no-example");
    assert_clean(&bare);

    let _ = std::fs::remove_dir_all(&temp);
}
