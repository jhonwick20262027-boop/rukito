//! Comportamiento de `rdd new` sobre el sistema de archivos (RF-140, RF-141).

use std::path::PathBuf;
use std::process::Command;

/// Directorio temporal propio de cada test, sin colisiones entre ellos.
fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rdd-cli-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("se debe poder crear el directorio temporal");
    dir
}

fn run(dir: &PathBuf, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_rdd"))
        .current_dir(dir)
        .args(args)
        .output()
        .expect("la CLI debe ejecutarse")
}

#[test]
fn dry_run_lists_without_writing() {
    let dir = temp_dir("dry-run");
    let output = run(&dir, &["new", "todo-app", "--dry-run"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("crearía"), "stdout: {stdout}");
    assert!(
        !dir.join("todo-app").exists(),
        "`--dry-run` no debe escribir"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn refuses_to_overwrite_without_force() {
    let dir = temp_dir("overwrite");
    assert!(run(&dir, &["new", "todo-app"]).status.success());

    let second = run(&dir, &["new", "todo-app"]);
    assert!(
        !second.status.success(),
        "RF-140: no debe sobrescribir sin --force"
    );
    let stderr = String::from_utf8_lossy(&second.stderr);
    assert!(stderr.contains("--force"), "stderr: {stderr}");

    let forced = run(&dir, &["new", "todo-app", "--force"]);
    assert!(
        forced.status.success(),
        "`--force` debe permitir sobrescribir"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn no_example_skips_the_tasks_module() {
    let dir = temp_dir("no-example");
    assert!(
        run(&dir, &["new", "todo-app", "--no-example"])
            .status
            .success()
    );
    assert!(dir.join("todo-app/src/main.rs").is_file());
    assert!(dir.join("todo-app/src/modules/mod.rs").is_file());
    assert!(
        !dir.join("todo-app/src/modules/tasks").exists(),
        "`--no-example` no debe generar el módulo"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Crea un proyecto y devuelve su raíz.
fn project(name: &str) -> PathBuf {
    let dir = temp_dir(name);
    assert!(run(&dir, &["new", "todo-app"]).status.success());
    dir.join("todo-app")
}

#[test]
fn new_generates_the_nestjs_tree() {
    let dir = temp_dir("tree");
    assert!(run(&dir, &["new", "todo-app"]).status.success());
    let root = dir.join("todo-app");
    for path in [
        "src/main.rs",
        "src/app_module.rs",
        "src/config/mod.rs",
        "src/common/guards/mod.rs",
        "src/common/middleware/mod.rs",
        "src/infrastructure/database/mod.rs",
        "src/integrations/payment_gateways/mod.rs",
        "src/shared/contracts/mod.rs",
        "src/generated/mod.rs",
        "src/modules/tasks/controllers/tasks_controller.rs",
        "src/modules/tasks/services/tasks_service.rs",
        "src/modules/tasks/repositories/tasks_repository.rs",
        "src/modules/tasks/dto/create_task_dto.rs",
        "src/modules/tasks/entities/task_entity.rs",
        "src/modules/tasks/interfaces/task_interface.rs",
        "src/modules/tasks/mappers/task_mapper.rs",
        "src/modules/tasks/constants/tasks_constants.rs",
        "src/modules/tasks/tests/tasks_service_test.rs",
        "tests/acceptance.rs",
        "prisma/schema.prisma",
        "docs/.gitkeep",
    ] {
        assert!(root.join(path).is_file(), "falta {path} (RF-002, RF-003)");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn g_module_generates_the_same_tree_and_registers_it() {
    let root = project("g-module");
    let output = run(&root, &["g", "module", "users"]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    for path in [
        "src/modules/users/mod.rs",
        "src/modules/users/controllers/users_controller.rs",
        "src/modules/users/services/users_service.rs",
        "src/modules/users/repositories/users_repository.rs",
        "src/modules/users/dto/create_user_dto.rs",
        "src/modules/users/dto/update_user_dto.rs",
        "src/modules/users/dto/query_user_dto.rs",
        "src/modules/users/entities/user_entity.rs",
        "src/modules/users/interfaces/user_interface.rs",
        "src/modules/users/mappers/user_mapper.rs",
        "src/modules/users/constants/users_constants.rs",
        "src/modules/users/tests/users_service_test.rs",
        "src/modules/users/tests/users_controller_test.rs",
    ] {
        assert!(root.join(path).is_file(), "falta {path} (RF-001)");
    }

    let mods = std::fs::read_to_string(root.join("src/modules/mod.rs")).unwrap();
    assert!(mods.contains("// rdd:mods"), "el marcador debe conservarse");
    assert!(mods.contains("pub mod users;"));
    assert!(mods.contains("pub mod tasks;"), "no debe pisar el ejemplo");

    let main = std::fs::read_to_string(root.join("src/main.rs")).unwrap();
    assert!(main.contains("// rdd:modules"));
    assert!(main.contains(".module(modules::users::module())"));
    assert!(main.contains(".module(modules::tasks::module())"));
    let _ = std::fs::remove_dir_all(root.parent().unwrap());
}

#[test]
fn g_module_normalizes_and_singularizes() {
    let root = project("g-module-naming");
    assert!(run(&root, &["g", "module", "TaskItems"]).status.success());
    assert!(root.join("src/modules/task_items/mod.rs").is_file());
    assert!(
        root.join("src/modules/task_items/dto/create_task_item_dto.rs")
            .is_file(),
        "RF-005: el singular va en dto/"
    );
    assert!(
        root.join("src/modules/task_items/services/task_items_service.rs")
            .is_file(),
        "RF-005: el plural se conserva en services/"
    );
    let controller = std::fs::read_to_string(
        root.join("src/modules/task_items/controllers/task_items_controller.rs"),
    )
    .unwrap();
    assert!(
        controller.contains("pub struct TaskItemsController;"),
        "RF-142"
    );
    assert!(controller.contains("#[controller(\"/task_items\")]"));
    let _ = std::fs::remove_dir_all(root.parent().unwrap());
}

#[test]
fn g_module_dry_run_writes_nothing() {
    let root = project("g-module-dry");
    let output = run(&root, &["g", "module", "users", "--dry-run"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("crearía"), "stdout: {stdout}");
    assert!(stdout.contains("modificaría"), "stdout: {stdout}");
    assert!(!root.join("src/modules/users").exists(), "RF-141");
    let mods = std::fs::read_to_string(root.join("src/modules/mod.rs")).unwrap();
    assert!(!mods.contains("pub mod users;"));
    let _ = std::fs::remove_dir_all(root.parent().unwrap());
}

#[test]
fn g_module_refuses_to_overwrite_without_force() {
    let root = project("g-module-overwrite");
    assert!(run(&root, &["g", "module", "users"]).status.success());
    let second = run(&root, &["g", "module", "users"]);
    assert!(!second.status.success(), "RF-140");
    assert!(
        String::from_utf8_lossy(&second.stderr).contains("--force"),
        "stderr: {}",
        String::from_utf8_lossy(&second.stderr)
    );
    assert!(
        run(&root, &["g", "module", "users", "--force"])
            .status
            .success()
    );
    let _ = std::fs::remove_dir_all(root.parent().unwrap());
}

#[test]
fn g_module_is_idempotent_in_the_markers() {
    let root = project("g-module-idempotent");
    assert!(run(&root, &["g", "module", "users"]).status.success());
    assert!(
        run(&root, &["g", "module", "users", "--force"])
            .status
            .success()
    );
    let mods = std::fs::read_to_string(root.join("src/modules/mod.rs")).unwrap();
    assert_eq!(mods.matches("pub mod users;").count(), 1);
    let main = std::fs::read_to_string(root.join("src/main.rs")).unwrap();
    assert_eq!(main.matches(".module(modules::users::module())").count(), 1);
    let _ = std::fs::remove_dir_all(root.parent().unwrap());
}

#[test]
fn g_module_without_marker_prints_the_lines_to_paste() {
    let root = project("g-module-no-marker");
    let main = std::fs::read_to_string(root.join("src/main.rs"))
        .unwrap()
        .replace("// rdd:modules", "// sin marcador");
    std::fs::write(root.join("src/main.rs"), main).unwrap();

    let output = run(&root, &["g", "module", "users"]);
    assert!(output.status.success(), "§14.3: no debe fallar");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("pega a mano"), "stdout: {stdout}");
    assert!(stdout.contains(".module(modules::users::module())"));
    assert!(root.join("src/modules/users/mod.rs").is_file());
    let _ = std::fs::remove_dir_all(root.parent().unwrap());
}
