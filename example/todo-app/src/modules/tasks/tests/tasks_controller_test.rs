//! Tabla de rutas que genera `#[controller]` (RF-051, RF-056).

use crate::modules::tasks::controllers::tasks_controller::TasksController;

#[test]
fn expone_las_rutas_del_modulo() {
    let table: Vec<String> = TasksController::routes()
        .iter()
        .map(|route| format!("{} {}", route.method, route.path))
        .collect();
    assert_eq!(
        table,
        [
            "GET /tasks",
            "GET /tasks/:id",
            "POST /tasks",
            "PUT /tasks/:id",
            "DELETE /tasks/:id",
        ]
    );
}
