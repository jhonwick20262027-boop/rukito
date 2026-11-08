//! Normalización de nombres (RF-142).
//!
//! `Tasks`, `tasks`, `task-items` y `TaskItems` se normalizan cada uno a la
//! grafía que pide el framework: módulo `snake_case`, tipo `PascalCase` y
//! paquete `kebab-case`.

/// Parte un nombre en palabras, separando por puntuación y por cambios de caja.
///
/// Reconoce `task-items`, `task_items`, `TaskItems` y `HTTPServer` (siglas).
pub fn split_words(input: &str) -> Vec<String> {
    let chars: Vec<char> = input.chars().collect();
    let mut words = Vec::new();
    let mut current = String::new();

    for (index, &ch) in chars.iter().enumerate() {
        if !ch.is_alphanumeric() {
            if !current.is_empty() {
                words.push(std::mem::take(&mut current));
            }
            continue;
        }

        if !current.is_empty() {
            let previous = chars[index - 1];
            let next = chars.get(index + 1).copied();
            let boundary = (ch.is_uppercase() && previous.is_lowercase())
                || (ch.is_uppercase()
                    && previous.is_uppercase()
                    && next.is_some_and(char::is_lowercase))
                || (ch.is_ascii_digit() && previous.is_alphabetic())
                || (ch.is_alphabetic() && previous.is_ascii_digit());
            if boundary {
                words.push(std::mem::take(&mut current));
            }
        }

        current.push(ch);
    }

    if !current.is_empty() {
        words.push(current);
    }
    words
}

/// `snake_case` para módulos, columnas y nombres de paquete internos.
pub fn snake_case(input: &str) -> String {
    split_words(input)
        .iter()
        .map(|word| word.to_lowercase())
        .collect::<Vec<_>>()
        .join("_")
}

/// `kebab-case` para el directorio y el nombre del paquete Cargo.
pub fn kebab_case(input: &str) -> String {
    split_words(input)
        .iter()
        .map(|word| word.to_lowercase())
        .collect::<Vec<_>>()
        .join("-")
}

/// Singular de un nombre de módulo (RF-005).
///
/// Se usa para `entities/`, `dto/`, `interfaces/` y `mappers/`; `controllers/`,
/// `services/`, `repositories/` y `constants/` conservan el plural. La regla es
/// fija, no lingüística: `ies`→`y`; `ses`/`xes`/`zes`/`ches`/`shes`→quita `es`;
/// `s` final salvo `ss`→quita `s`; en otro caso, igual.
pub fn singular(input: &str) -> String {
    // La terminación decide la regla, pero lo que se quita son solo los `es`
    // finales: `classes` → `class`, no `clas`.
    if ["shes", "ches", "ses", "xes", "zes"]
        .iter()
        .any(|suffix| input.ends_with(suffix))
    {
        if let Some(stem) = input.strip_suffix("es") {
            return stem.to_owned();
        }
    }
    if let Some(stem) = input.strip_suffix("ies") {
        return format!("{stem}y");
    }
    if !input.ends_with("ss") {
        if let Some(stem) = input.strip_suffix('s') {
            return stem.to_owned();
        }
    }
    input.to_owned()
}

/// `PascalCase` para tipos y nombres de controller.
pub fn pascal_case(input: &str) -> String {
    split_words(input)
        .iter()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => {
                    let mut out = first.to_uppercase().collect::<String>();
                    out.push_str(&chars.as_str().to_lowercase());
                    out
                }
                None => String::new(),
            }
        })
        .collect::<String>()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_common_spellings() {
        assert_eq!(snake_case("Tasks"), "tasks");
        assert_eq!(snake_case("tasks"), "tasks");
        assert_eq!(snake_case("task-items"), "task_items");
        assert_eq!(snake_case("TaskItems"), "task_items");
        assert_eq!(kebab_case("TaskItems"), "task-items");
        assert_eq!(pascal_case("task_items"), "TaskItems");
    }

    #[test]
    fn splits_camel_case_and_acronyms() {
        assert_eq!(split_words("HTTPServer"), ["HTTP", "Server"]);
        assert_eq!(split_words("TaskItems"), ["Task", "Items"]);
        assert_eq!(split_words("task-items"), ["task", "items"]);
        assert_eq!(split_words("task_items"), ["task", "items"]);
    }

    #[test]
    fn produces_the_three_cases() {
        assert_eq!(snake_case("HTTPServer"), "http_server");
        assert_eq!(kebab_case("HTTPServer"), "http-server");
        assert_eq!(pascal_case("HTTPServer"), "HttpServer");
        assert_eq!(pascal_case("todo-app"), "TodoApp");
    }

    #[test]
    fn keeps_digits_inside_a_word_group() {
        assert_eq!(snake_case("Task2Item"), "task_2_item");
    }

    #[test]
    fn singularizes_by_the_documented_rule() {
        assert_eq!(singular("tasks"), "task");
        assert_eq!(singular("task_items"), "task_item");
        assert_eq!(singular("categories"), "category");
        assert_eq!(singular("classes"), "class");
        assert_eq!(singular("boxes"), "box");
        assert_eq!(singular("watches"), "watch");
        assert_eq!(singular("dishes"), "dish");
        assert_eq!(singular("address"), "address");
        assert_eq!(singular("task"), "task");
    }
}
