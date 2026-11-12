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
fn migrate_applies_versioned_sql_and_is_idempotent() {
    let dir = temp_dir("migrate");
    std::fs::write(
        dir.join("Cargo.toml"),
        "[package]\nname = \"migration-test\"\n",
    )
    .expect("manifest must be created");
    std::fs::write(
        dir.join("rdd.toml"),
        "[database]\nmigrations_dir = \"db/changes\"\n",
    )
    .expect("config must be created");
    let migrations = dir.join("db/changes");
    std::fs::create_dir_all(&migrations).expect("migration directory must be created");
    std::fs::write(
        migrations.join("0001_create_widgets.sql"),
        "CREATE TABLE widgets (id INTEGER PRIMARY KEY, title TEXT NOT NULL);",
    )
    .expect("migration must be created");

    let first = run(
        &dir,
        &["migrate", "--database-url", "sqlite://migration-test.db"],
    );
    assert!(
        first.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&first.stderr)
    );
    assert!(
        String::from_utf8_lossy(&first.stdout).contains("create widgets"),
        "stdout: {}",
        String::from_utf8_lossy(&first.stdout)
    );

    let second = run(
        &dir,
        &["migrate", "--database-url", "sqlite://migration-test.db"],
    );
    assert!(
        second.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&second.stderr)
    );
    assert!(
        String::from_utf8_lossy(&second.stdout).contains("al día"),
        "stdout: {}",
        String::from_utf8_lossy(&second.stdout)
    );

    std::fs::write(
        migrations.join("0001_create_widgets.sql"),
        "CREATE TABLE widgets (id INTEGER PRIMARY KEY, title TEXT NOT NULL, done BOOLEAN NOT NULL DEFAULT FALSE);",
    )
    .expect("migration must be changed");
    let changed = run(
        &dir,
        &["migrate", "--database-url", "sqlite://migration-test.db"],
    );
    assert!(
        !changed.status.success(),
        "changing an applied migration must fail"
    );

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime must be created");
    runtime.block_on(async {
        let options = sqlx::sqlite::SqliteConnectOptions::new()
            .filename(dir.join("migration-test.db"))
            .create_if_missing(false);
        let db = sqlx::sqlite::SqlitePoolOptions::new()
            .connect_with(options)
            .await
            .expect("SQLite DB must exist");
        let exists: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'widgets'",
        )
        .fetch_one(&db)
        .await
        .expect("migration table must be queryable");
        assert_eq!(exists, 1);
        db.close().await;
    });

    let _ = std::fs::remove_dir_all(&dir);
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

/// RF-189: el flag llega hasta los archivos, y un motor sin feature en la
/// fachada se rechaza antes de escribir nada.
#[test]
fn new_database_flag_reaches_the_files_and_rejects_unsupported_engines() {
    let dir = temp_dir("database");
    let default = run(&dir, &["new", "default-app"]);
    assert!(default.status.success());
    let manifest = std::fs::read_to_string(dir.join("default-app/Cargo.toml")).unwrap();
    assert!(
        manifest.contains("rdd = { package = \"rdd-framework\", version = \"0.1\" }"),
        "sin el flag la fachada va con sus features por defecto: {manifest}"
    );

    let pg = run(&dir, &["new", "pg-app", "--database", "postgres"]);
    assert!(
        pg.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&pg.stderr)
    );
    let manifest = std::fs::read_to_string(dir.join("pg-app/Cargo.toml")).unwrap();
    assert!(manifest.contains("features = [\"postgres\"]"), "{manifest}");
    let config = std::fs::read_to_string(dir.join("pg-app/rdd.toml")).unwrap();
    assert!(config.contains("provider = \"postgres\""), "{config}");

    let bad = run(&dir, &["new", "mysql-app", "--database", "mysql"]);
    assert!(
        !bad.status.success(),
        "mysql todavía no tiene feature en la fachada"
    );
    let stderr = String::from_utf8_lossy(&bad.stderr);
    assert!(
        stderr.contains("postgres") && stderr.contains("sqlite"),
        "debe nombrar los motores disponibles: {stderr}"
    );
    assert!(
        !dir.join("mysql-app").exists(),
        "un flag inválido no debe escribir nada"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn new_generates_the_minimal_functional_tree() {
    let dir = temp_dir("tree");
    assert!(run(&dir, &["new", "todo-app"]).status.success());
    let root = dir.join("todo-app");
    for path in [
        "src/main.rs",
        "src/app_module.rs",
        "src/config/mod.rs",
        "src/config/app_config.rs",
        "src/config/database_config.rs",
        "src/generated/mod.rs",
        "src/modules/tasks/mod.rs",
        "src/modules/tasks/domain/task.rs",
        "src/modules/tasks/domain/tasks_repository.rs",
        "src/modules/tasks/application/mod.rs",
        "src/modules/tasks/application/tasks_use_cases.rs",
        "src/modules/tasks/infrastructure/mod.rs",
        "src/modules/tasks/infrastructure/tasks_repository.rs",
        "src/modules/tasks/presentation/mod.rs",
        "src/modules/tasks/presentation/dto.rs",
        "src/modules/tasks/presentation/tasks_controller.rs",
        "tests/acceptance.rs",
        "prisma/schema.prisma",
    ] {
        assert!(root.join(path).is_file(), "falta {path} (RF-002, RF-003)");
    }
    for path in [
        "src/common",
        "src/infrastructure",
        "src/integrations",
        "src/shared",
        "src/config/jwt_config.rs",
        "src/config/redis_config.rs",
        "tests/e2e",
        "tests/fixtures",
        "docs",
    ] {
        assert!(
            !root.join(path).exists(),
            "no debe generar el placeholder {path}"
        );
    }
    for folder in [
        "controllers",
        "repositories",
        "dto",
        "entities",
        "interfaces",
        "mappers",
        "constants",
        "tests",
        "services",
    ] {
        assert!(
            !root.join(format!("src/modules/tasks/{folder}")).exists(),
            "no debe generar la carpeta redundante {folder}"
        );
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
        "src/modules/users/domain/user.rs",
        "src/modules/users/domain/users_repository.rs",
        "src/modules/users/application/mod.rs",
        "src/modules/users/application/users_use_cases.rs",
        "src/modules/users/infrastructure/mod.rs",
        "src/modules/users/infrastructure/users_repository.rs",
        "src/modules/users/presentation/mod.rs",
        "src/modules/users/presentation/dto.rs",
        "src/modules/users/presentation/users_controller.rs",
    ] {
        assert!(root.join(path).is_file(), "falta {path} (RF-001)");
    }
    for folder in [
        "controllers",
        "repositories",
        "dto",
        "entities",
        "interfaces",
        "mappers",
        "constants",
        "tests",
        "services",
    ] {
        assert!(
            !root.join(format!("src/modules/users/{folder}")).exists(),
            "no debe generar la carpeta redundante {folder}"
        );
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
        root.join("src/modules/task_items/presentation/dto.rs")
            .is_file(),
        "los DTO se agrupan en presentation/"
    );
    assert!(
        root.join("src/modules/task_items/application/task_items_use_cases.rs")
            .is_file(),
        "el nombre plural se conserva en application/"
    );
    let controller = std::fs::read_to_string(
        root.join("src/modules/task_items/presentation/task_items_controller.rs"),
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
