//! Validación del documento y expansión de la selección (`RF-235`, `RF-236`).
//!
//! Se valida antes de ejecutar: campos y argumentos, variables y su
//! compatibilidad, fragmentos (existencia y ciclos), directivas y profundidad.
//! La expansión resuelve alias, fragmentos y `@skip`/`@include` una sola vez.

use std::collections::{HashMap, HashSet};

use serde_json::{Map, Value};

use crate::parser::{
    self, Argument, Directive, Document, Field, Operation, OperationKind, Selection,
};
use crate::schema::{Root, Schema, select_operation};
use crate::types::{
    ArgDef, InputObjectDef, ObjectTypeDef, RootKind, SelectedField, SelectionSet, TypeRef,
};

pub(crate) use crate::schema::GraphqlError;

/// Documento ya validado y selección raíz expandida.
pub(crate) struct Prepared {
    /// Tipo de operación.
    pub kind: OperationKind,
    /// Selección raíz lista para ejecutar.
    pub selection: SelectionSet,
}

/// Todo lo que la expansión necesita leer de la petición.
#[derive(Clone, Copy)]
struct Inputs<'a> {
    schema: &'a Schema,
    document: &'a Document,
    variables: &'a HashMap<String, Value>,
    variable_types: &'a HashMap<String, TypeRef>,
    max_depth: usize,
}

/// Valida y expande la operación elegida (`RF-235`, `RF-236`).
pub(crate) fn analyze(
    schema: &Schema,
    req: &crate::GraphqlRequest,
    max_depth: usize,
) -> Result<Prepared, Vec<GraphqlError>> {
    let document = parser::parse(&req.query)
        .map_err(|err| vec![GraphqlError::at(err.message, err.position)])?;
    let operation =
        select_operation(&document, req.operation_name.as_deref()).map_err(|err| vec![err])?;

    let mut errors = Vec::new();
    check_operations(&document, &mut errors);
    check_fragment_cycles(&document, &mut errors);
    if !errors.is_empty() {
        return Err(errors);
    }
    if operation.selection.is_empty() {
        return Err(vec![GraphqlError::at(
            "la operación no selecciona ningún campo",
            operation.position,
        )]);
    }

    let variables = coerce_variables(schema, operation, req.variables.as_ref(), &mut errors);
    let variable_types: HashMap<String, TypeRef> = operation
        .variables
        .iter()
        .map(|variable| (variable.name.clone(), variable.ty.clone()))
        .collect();
    let Some(root) = schema.root(operation.kind.root_kind()) else {
        return Err(vec![GraphqlError::at(
            format!(
                "el esquema no publica una raíz de {}",
                operation.kind.keyword()
            ),
            operation.position,
        )]);
    };

    let inputs = Inputs {
        schema,
        document: &document,
        variables: &variables,
        variable_types: &variable_types,
        max_depth,
    };
    let mut analysis = Analysis { errors };
    let selection = analysis.expand(
        &inputs,
        Parent::Root(root, operation.kind.root_kind()),
        &operation.selection,
        1,
        true,
        &[],
    );
    let mut errors = analysis.errors;

    if operation.kind == OperationKind::Subscription {
        let fields = selection
            .fields
            .iter()
            .filter(|field| !is_introspection(&field.name))
            .count();
        if fields != 1 {
            errors.push(GraphqlError::at(
                "una suscripción debe seleccionar un único campo raíz",
                operation.position,
            ));
        }
    }

    if errors.is_empty() {
        Ok(Prepared {
            kind: operation.kind,
            selection,
        })
    } else {
        Err(errors)
    }
}

fn is_introspection(name: &str) -> bool {
    name == "__schema" || name == "__type"
}

fn check_operations(document: &Document, errors: &mut Vec<GraphqlError>) {
    let mut names: HashMap<&str, parser::Position> = HashMap::new();
    let mut anonymous = 0;
    for operation in &document.operations {
        match operation.name.as_deref() {
            Some(name) => {
                if names.insert(name, operation.position).is_some() {
                    errors.push(GraphqlError::at(
                        format!("la operación `{name}` está duplicada"),
                        operation.position,
                    ));
                }
            }
            None => anonymous += 1,
        }
    }
    if anonymous > 1 {
        errors.push(GraphqlError::message(
            "el documento tiene más de una operación anónima",
        ));
    }
}

fn check_fragment_cycles(document: &Document, errors: &mut Vec<GraphqlError>) {
    let mut done: HashSet<String> = HashSet::new();
    let mut visiting: Vec<String> = Vec::new();
    for name in document.fragments.keys() {
        visit_fragment(name, document, &mut visiting, &mut done, errors);
    }
}

fn visit_fragment(
    name: &str,
    document: &Document,
    visiting: &mut Vec<String>,
    done: &mut HashSet<String>,
    errors: &mut Vec<GraphqlError>,
) {
    if done.contains(name) {
        return;
    }
    if visiting.iter().any(|item| item == name) {
        let position = document
            .fragments
            .get(name)
            .map(|fragment| fragment.position)
            .unwrap_or(parser::Position { line: 1, column: 1 });
        errors.push(GraphqlError::at(
            format!("el fragmento `{name}` es cíclico"),
            position,
        ));
        return;
    }
    let Some(fragment) = document.fragments.get(name) else {
        return;
    };
    visiting.push(name.to_owned());
    let mut spreads = Vec::new();
    collect_spreads(&fragment.selection, &mut spreads);
    for spread in spreads {
        visit_fragment(&spread, document, visiting, done, errors);
    }
    visiting.pop();
    done.insert(name.to_owned());
}

fn collect_spreads(selection: &[Selection], out: &mut Vec<String>) {
    for item in selection {
        match item {
            Selection::Field(field) => collect_spreads(&field.selection, out),
            Selection::FragmentSpread(spread) => out.push(spread.name.clone()),
            Selection::InlineFragment(inline) => collect_spreads(&inline.selection, out),
        }
    }
}

fn coerce_variables(
    schema: &Schema,
    operation: &Operation,
    provided: Option<&Value>,
    errors: &mut Vec<GraphqlError>,
) -> HashMap<String, Value> {
    let mut values = HashMap::new();
    let provided = match provided {
        None | Some(Value::Null) => Map::new(),
        Some(Value::Object(map)) => map.clone(),
        Some(_) => {
            errors.push(GraphqlError::at(
                "`variables` debe ser un objeto JSON",
                operation.position,
            ));
            Map::new()
        }
    };

    for variable in &operation.variables {
        let raw = provided
            .get(&variable.name)
            .cloned()
            .or_else(|| {
                variable
                    .default
                    .as_ref()
                    .map(|value| literal_to_json(value, &HashMap::new()).unwrap_or(Value::Null))
            })
            .unwrap_or(Value::Null);
        if raw.is_null() && variable.ty.is_non_null() {
            errors.push(GraphqlError::at(
                format!(
                    "la variable `${}` es obligatoria (tipo `{}`)",
                    variable.name, variable.ty
                ),
                variable.position,
            ));
            continue;
        }
        match coerce_value(schema, &variable.ty, &raw) {
            Ok(value) => {
                values.insert(variable.name.clone(), value);
            }
            Err(err) => errors.push(GraphqlError::at(
                format!("la variable `${}`: {err}", variable.name),
                variable.position,
            )),
        }
    }
    values
}

fn literal_to_json(
    value: &parser::Value,
    variables: &HashMap<String, Value>,
) -> Result<Value, String> {
    match value {
        parser::Value::Null => Ok(Value::Null),
        parser::Value::Boolean(flag) => Ok(Value::Bool(*flag)),
        parser::Value::Integer(number) => Ok(Value::Number((*number).into())),
        parser::Value::Float(number) => Ok(serde_json::Number::from_f64(*number)
            .map(Value::Number)
            .unwrap_or(Value::Null)),
        parser::Value::String(text) => Ok(Value::String(text.clone())),
        parser::Value::Variable(name) => variables
            .get(name)
            .cloned()
            .ok_or_else(|| format!("la variable `${name}` no está declarada")),
        parser::Value::List(items) => items
            .iter()
            .map(|item| literal_to_json(item, variables))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        parser::Value::Object(entries) => {
            let mut map = Map::new();
            for (name, item) in entries {
                map.insert(name.clone(), literal_to_json(item, variables)?);
            }
            Ok(Value::Object(map))
        }
        parser::Value::Enum(name) => Err(format!(
            "el valor de enum `{name}` no está soportado: se esperaba un literal"
        )),
    }
}

/// Coerciona un valor ya en JSON al tipo del esquema (`RF-236`).
fn coerce_value(schema: &Schema, ty: &TypeRef, value: &Value) -> Result<Value, String> {
    match ty {
        TypeRef::NonNull(inner) => {
            if value.is_null() {
                Err("se esperaba un valor no nulo".to_owned())
            } else {
                coerce_value(schema, inner, value)
            }
        }
        _ if value.is_null() => Ok(Value::Null),
        TypeRef::List(inner) => match value {
            Value::Array(items) => items
                .iter()
                .map(|item| coerce_value(schema, inner, item))
                .collect::<Result<Vec<_>, _>>()
                .map(Value::Array),
            other => coerce_value(schema, inner, other).map(|item| Value::Array(vec![item])),
        },
        TypeRef::Named(name) => coerce_named(schema, name, value),
    }
}

fn coerce_named(schema: &Schema, name: &str, value: &Value) -> Result<Value, String> {
    match name {
        "String" => value
            .as_str()
            .map(|text| Value::String(text.to_owned()))
            .ok_or_else(|| "se esperaba una cadena".to_owned()),
        "Boolean" => value
            .as_bool()
            .map(Value::Bool)
            .ok_or_else(|| "se esperaba un booleano".to_owned()),
        "Int" => value
            .as_i64()
            .and_then(|number| i32::try_from(number).ok())
            .map(|number| Value::Number(number.into()))
            .ok_or_else(|| "se esperaba un entero de 32 bits".to_owned()),
        "Float" => value
            .as_f64()
            .and_then(serde_json::Number::from_f64)
            .map(Value::Number)
            .ok_or_else(|| "se esperaba un número".to_owned()),
        "ID" => match value {
            Value::String(text) => Ok(Value::String(text.clone())),
            Value::Number(number) => Ok(Value::String(number.to_string())),
            _ => Err("se esperaba un ID en texto o número".to_owned()),
        },
        _ => {
            let Some(input) = schema.input(name) else {
                return Err(format!("el tipo de entrada `{name}` no existe"));
            };
            coerce_input_object(schema, input, value)
        }
    }
}

fn coerce_input_object(
    schema: &Schema,
    input: &InputObjectDef,
    value: &Value,
) -> Result<Value, String> {
    let Value::Object(map) = value else {
        return Err(format!("se esperaba un objeto `{}`", input.name));
    };
    for key in map.keys() {
        if !input.fields.iter().any(|field| field.name == key) {
            return Err(format!("campo desconocido `{key}` en `{}`", input.name));
        }
    }
    let mut out = Map::new();
    for field in &input.fields {
        let raw = map
            .get(field.name)
            .or(field.default.as_ref())
            .cloned()
            .unwrap_or(Value::Null);
        if raw.is_null() && field.ty.is_non_null() {
            return Err(format!(
                "falta el campo obligatorio `{}` en `{}`",
                field.name, input.name
            ));
        }
        let coerced = coerce_value(schema, &field.ty, &raw)
            .map_err(|err| format!("campo `{}` de `{}`: {err}", field.name, input.name))?;
        if !coerced.is_null() {
            out.insert(field.name.to_owned(), coerced);
        }
    }
    Ok(Value::Object(out))
}

/// Tipo al que se aplica una selección: una raíz o un objeto publicado.
#[derive(Clone, Copy)]
enum Parent<'a> {
    Root(&'a Root, RootKind),
    Object(&'a ObjectTypeDef),
}

impl Parent<'_> {
    fn name(&self) -> &str {
        match self {
            Self::Root(_, kind) => kind.type_name(),
            Self::Object(object) => object.name,
        }
    }

    fn field(&self, name: &str) -> Option<FieldMeta<'_>> {
        match self {
            Self::Root(root, _) => {
                root.fields
                    .iter()
                    .find(|field| field.name == name)
                    .map(|field| FieldMeta {
                        ty: &field.ty,
                        args: &field.args,
                    })
            }
            Self::Object(object) => {
                object
                    .fields
                    .iter()
                    .find(|field| field.name == name)
                    .map(|field| FieldMeta {
                        ty: &field.ty,
                        args: &field.args,
                    })
            }
        }
    }
}

struct FieldMeta<'a> {
    ty: &'a TypeRef,
    args: &'a [ArgDef],
}

struct Analysis {
    errors: Vec<GraphqlError>,
}

impl Analysis {
    fn expand(
        &mut self,
        input: &Inputs<'_>,
        parent: Parent<'_>,
        selection: &[Selection],
        depth: usize,
        included: bool,
        path: &[String],
    ) -> SelectionSet {
        let mut out = SelectionSet::default();
        if depth > input.max_depth {
            if let Some(position) = first_position(selection) {
                self.errors.push(GraphqlError::at(
                    format!(
                        "la consulta supera la profundidad máxima de {}",
                        input.max_depth
                    ),
                    position,
                ));
            }
            return out;
        }

        for item in selection {
            match item {
                Selection::Field(field) if field.name == "__typename" => {
                    self.expand_typename(input, field, included, path, &mut out);
                }
                Selection::Field(field) => {
                    if let Some(selected) =
                        self.expand_field(input, parent, field, depth, included, path)
                    {
                        out.fields.push(selected);
                    }
                }
                Selection::FragmentSpread(spread) => {
                    let include = directives_include(
                        &spread.directives,
                        input.variables,
                        input.variable_types,
                        &mut self.errors,
                    );
                    let Some(fragment) = input.document.fragments.get(&spread.name) else {
                        self.errors.push(GraphqlError::at(
                            format!("fragmento desconocido `{}`", spread.name),
                            spread.position,
                        ));
                        continue;
                    };
                    if fragment.type_condition != parent.name() {
                        self.errors.push(GraphqlError::at(
                            format!(
                                "el fragmento `{}` es de `{}` y no aplica a `{}`",
                                spread.name,
                                fragment.type_condition,
                                parent.name()
                            ),
                            spread.position,
                        ));
                        continue;
                    }
                    let fragment_include = directives_include(
                        &fragment.directives,
                        input.variables,
                        input.variable_types,
                        &mut self.errors,
                    );
                    let mut nested = self.expand(
                        input,
                        parent,
                        &fragment.selection,
                        depth,
                        included && include && fragment_include,
                        path,
                    );
                    out.fields.append(&mut nested.fields);
                    out.typename_keys.append(&mut nested.typename_keys);
                }
                Selection::InlineFragment(inline) => {
                    let include = directives_include(
                        &inline.directives,
                        input.variables,
                        input.variable_types,
                        &mut self.errors,
                    );
                    if let Some(condition) = &inline.type_condition
                        && condition != parent.name()
                    {
                        self.errors.push(GraphqlError::at(
                            format!(
                                "el fragmento inline es de `{condition}` y no aplica a `{}`",
                                parent.name()
                            ),
                            inline.position,
                        ));
                        continue;
                    }
                    let mut nested = self.expand(
                        input,
                        parent,
                        &inline.selection,
                        depth,
                        included && include,
                        path,
                    );
                    out.fields.append(&mut nested.fields);
                    out.typename_keys.append(&mut nested.typename_keys);
                }
            }
        }
        out
    }

    fn expand_typename(
        &mut self,
        input: &Inputs<'_>,
        field: &Field,
        included: bool,
        path: &[String],
        out: &mut SelectionSet,
    ) {
        let key = field.alias.clone().unwrap_or_else(|| field.name.clone());
        let child_path = child_path(path, &key);
        let include = directives_include(
            &field.directives,
            input.variables,
            input.variable_types,
            &mut self.errors,
        );
        if !field.arguments.is_empty() {
            self.errors.push(
                GraphqlError::at("`__typename` no admite argumentos", field.position)
                    .with_path(child_path.clone()),
            );
        }
        if !field.selection.is_empty() {
            self.errors.push(
                GraphqlError::at("`__typename` no admite selección", field.position)
                    .with_path(child_path),
            );
        }
        if include && included {
            out.typename_keys.push(key);
        }
    }

    fn expand_field(
        &mut self,
        input: &Inputs<'_>,
        parent: Parent<'_>,
        field: &Field,
        depth: usize,
        included: bool,
        path: &[String],
    ) -> Option<SelectedField> {
        let key = field.alias.clone().unwrap_or_else(|| field.name.clone());
        let child_path = child_path(path, &key);
        let include = directives_include(
            &field.directives,
            input.variables,
            input.variable_types,
            &mut self.errors,
        );

        if is_introspection(&field.name) {
            return self.expand_introspection(
                input,
                parent,
                field,
                depth,
                include && included,
                key,
                child_path,
            );
        }

        let Some(meta) = parent.field(&field.name) else {
            self.errors.push(
                GraphqlError::at(
                    format!("campo desconocido `{}` en `{}`", field.name, parent.name()),
                    field.position,
                )
                .with_path(child_path),
            );
            return None;
        };

        let mut args = Map::new();
        for argument in &field.arguments {
            if !meta.args.iter().any(|def| def.name == argument.name) {
                self.errors.push(
                    GraphqlError::at(
                        format!(
                            "argumento desconocido `{}` en `{}`",
                            argument.name, field.name
                        ),
                        argument.position,
                    )
                    .with_path(child_path.clone()),
                );
            }
        }
        for def in meta.args {
            if let Some(argument) = field
                .arguments
                .iter()
                .find(|argument| argument.name == def.name)
            {
                match argument_value(input, def, argument) {
                    Ok(value) => {
                        args.insert(def.name.to_owned(), value);
                    }
                    Err(message) => self.errors.push(
                        GraphqlError::at(message, argument.position).with_path(child_path.clone()),
                    ),
                }
            } else if def.ty.is_non_null() {
                self.errors.push(
                    GraphqlError::at(
                        format!(
                            "falta el argumento obligatorio `{}` en `{}`",
                            def.name, field.name
                        ),
                        field.position,
                    )
                    .with_path(child_path.clone()),
                );
            }
        }

        let named = meta.ty.named_type().to_owned();
        let child_selection = if crate::schema::SCALARS.contains(&named.as_str()) {
            if !field.selection.is_empty() {
                self.errors.push(
                    GraphqlError::at(
                        format!("el campo `{}` es escalar y no admite selección", field.name),
                        field.position,
                    )
                    .with_path(child_path.clone()),
                );
            }
            SelectionSet::default()
        } else {
            let Some(object) = input.schema.object(&named) else {
                self.errors.push(
                    GraphqlError::at(
                        format!("el tipo `{named}` no está definido"),
                        field.position,
                    )
                    .with_path(child_path),
                );
                return None;
            };
            if field.selection.is_empty() {
                self.errors.push(
                    GraphqlError::at(
                        format!("el campo `{}` requiere una selección", field.name),
                        field.position,
                    )
                    .with_path(child_path.clone()),
                );
                SelectionSet::default()
            } else {
                self.expand(
                    input,
                    Parent::Object(object),
                    &field.selection,
                    depth + 1,
                    true,
                    &child_path,
                )
            }
        };

        if include && included {
            Some(SelectedField {
                key,
                name: field.name.clone(),
                args: Value::Object(args),
                selection: child_selection,
            })
        } else {
            None
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn expand_introspection(
        &mut self,
        input: &Inputs<'_>,
        parent: Parent<'_>,
        field: &Field,
        depth: usize,
        included: bool,
        key: String,
        _child_path: Vec<String>,
    ) -> Option<SelectedField> {
        if !matches!(parent, Parent::Root(_, RootKind::Query)) {
            self.errors.push(GraphqlError::at(
                format!("`{}` solo existe en la raíz de consulta", field.name),
                field.position,
            ));
            return None;
        }

        let mut args = Map::new();
        if field.name == "__schema" {
            if !field.arguments.is_empty() {
                self.errors.push(GraphqlError::at(
                    "`__schema` no admite argumentos",
                    field.position,
                ));
            }
        } else {
            match arguments_json(&field.arguments, input.variables) {
                Ok(value) => {
                    let Some(name) = value.get("name").and_then(Value::as_str) else {
                        self.errors.push(GraphqlError::at(
                            "`__type` exige el argumento `name: String!`",
                            field.position,
                        ));
                        return None;
                    };
                    args.insert("name".to_owned(), Value::String(name.to_owned()));
                }
                Err((message, position)) => {
                    self.errors.push(GraphqlError::at(message, position));
                    return None;
                }
            }
        }

        if !included {
            return None;
        }
        let selection = permissive_selection(
            &field.selection,
            input.variables,
            input.variable_types,
            input.document,
            depth + 1,
            input.max_depth,
            &mut self.errors,
        );
        Some(SelectedField {
            key,
            name: field.name.clone(),
            args: Value::Object(args),
            selection,
        })
    }
}

fn child_path(path: &[String], key: &str) -> Vec<String> {
    let mut child = path.to_vec();
    child.push(key.to_owned());
    child
}

fn first_position(selection: &[Selection]) -> Option<parser::Position> {
    selection.first().map(|item| match item {
        Selection::Field(field) => field.position,
        Selection::FragmentSpread(spread) => spread.position,
        Selection::InlineFragment(inline) => inline.position,
    })
}

fn argument_value(input: &Inputs<'_>, def: &ArgDef, argument: &Argument) -> Result<Value, String> {
    if let Some(name) = argument.value.variable() {
        let Some(declared) = input.variable_types.get(name) else {
            return Err(format!("la variable `${name}` no está declarada"));
        };
        if !variable_compatible(declared, &def.ty) {
            return Err(format!(
                "la variable `${name}` de tipo `{declared}` no encaja con el argumento `{}: {}`",
                def.name, def.ty
            ));
        }
        let Some(value) = input.variables.get(name) else {
            return Err(format!("la variable `${name}` no tiene valor"));
        };
        return coerce_value(input.schema, &def.ty, value);
    }
    let value = literal_to_json(&argument.value, input.variables)
        .map_err(|err| format!("argumento `{}`: {err}", def.name))?;
    coerce_value(input.schema, &def.ty, &value)
}

/// Regla de compatibilidad de GraphQL: un tipo declarado encaja en la posición
/// si es el mismo, o si es no-null donde se espera nullable; el no-null del
/// destino exige no-null en la variable.
fn variable_compatible(declared: &TypeRef, location: &TypeRef) -> bool {
    match (declared, location) {
        (TypeRef::NonNull(declared), TypeRef::NonNull(location)) => {
            variable_compatible(declared, location)
        }
        (_, TypeRef::NonNull(_)) => false,
        (TypeRef::NonNull(declared), location) => variable_compatible(declared, location),
        (TypeRef::List(declared), TypeRef::List(location)) => {
            variable_compatible(declared, location)
        }
        (TypeRef::Named(declared), TypeRef::Named(location)) => declared == location,
        _ => false,
    }
}

fn directives_include(
    directives: &[Directive],
    variables: &HashMap<String, Value>,
    variable_types: &HashMap<String, TypeRef>,
    errors: &mut Vec<GraphqlError>,
) -> bool {
    let mut include = true;
    for directive in directives {
        match directive.name.as_str() {
            "skip" | "include" => {
                for argument in &directive.arguments {
                    if argument.name != "if" {
                        errors.push(GraphqlError::at(
                            format!(
                                "la directiva `@{}` no admite el argumento `{}`",
                                directive.name, argument.name
                            ),
                            argument.position,
                        ));
                    }
                }
                let Some(argument) = directive
                    .arguments
                    .iter()
                    .find(|argument| argument.name == "if")
                else {
                    errors.push(GraphqlError::at(
                        format!("la directiva `@{}` exige `if`", directive.name),
                        directive.position,
                    ));
                    continue;
                };
                if let Some(name) = argument.value.variable() {
                    match variable_types.get(name) {
                        Some(declared)
                            if !variable_compatible(declared, &TypeRef::required("Boolean")) =>
                        {
                            errors.push(GraphqlError::at(
                                format!(
                                    "`@{}(if:)` exige una variable `Boolean!`, no `${name}`",
                                    directive.name
                                ),
                                argument.position,
                            ));
                            continue;
                        }
                        None => {
                            errors.push(GraphqlError::at(
                                format!("la variable `${name}` no está declarada"),
                                argument.position,
                            ));
                            continue;
                        }
                        Some(_) => {}
                    }
                }
                match literal_to_json(&argument.value, variables) {
                    Ok(Value::Bool(flag)) => {
                        if directive.name == "skip" && flag {
                            include = false;
                        }
                        if directive.name == "include" && !flag {
                            include = false;
                        }
                    }
                    Ok(_) => errors.push(GraphqlError::at(
                        format!("`@{}(if:)` espera un booleano", directive.name),
                        argument.position,
                    )),
                    Err(message) => {
                        errors.push(GraphqlError::at(message, argument.position));
                    }
                }
            }
            other => errors.push(GraphqlError::at(
                format!("directiva no admitida `@{other}`"),
                directive.position,
            )),
        }
    }
    include
}

fn arguments_json(
    arguments: &[Argument],
    variables: &HashMap<String, Value>,
) -> Result<Value, (String, parser::Position)> {
    let mut map = Map::new();
    for argument in arguments {
        let value = literal_to_json(&argument.value, variables)
            .map_err(|message| (message, argument.position))?;
        map.insert(argument.name.clone(), value);
    }
    Ok(Value::Object(map))
}

/// Selección sin validación de esquema, para la introspección (`RF-237`).
#[allow(clippy::too_many_arguments)]
fn permissive_selection(
    selection: &[Selection],
    variables: &HashMap<String, Value>,
    variable_types: &HashMap<String, TypeRef>,
    document: &Document,
    depth: usize,
    max_depth: usize,
    errors: &mut Vec<GraphqlError>,
) -> SelectionSet {
    let mut out = SelectionSet::default();
    if depth > max_depth {
        if let Some(position) = first_position(selection) {
            errors.push(GraphqlError::at(
                format!("la consulta supera la profundidad máxima de {max_depth}"),
                position,
            ));
        }
        return out;
    }
    for item in selection {
        match item {
            Selection::Field(field) => {
                let include =
                    directives_include(&field.directives, variables, variable_types, errors);
                let key = field.alias.clone().unwrap_or_else(|| field.name.clone());
                if !include {
                    continue;
                }
                if field.name == "__typename" {
                    out.typename_keys.push(key);
                    continue;
                }
                let args = match arguments_json(&field.arguments, variables) {
                    Ok(args) => args,
                    Err((message, position)) => {
                        errors.push(GraphqlError::at(message, position));
                        Value::Object(Map::new())
                    }
                };
                out.fields.push(SelectedField {
                    key,
                    name: field.name.clone(),
                    args,
                    selection: permissive_selection(
                        &field.selection,
                        variables,
                        variable_types,
                        document,
                        depth + 1,
                        max_depth,
                        errors,
                    ),
                });
            }
            Selection::FragmentSpread(spread) => {
                let include =
                    directives_include(&spread.directives, variables, variable_types, errors);
                if !include {
                    continue;
                }
                let Some(fragment) = document.fragments.get(&spread.name) else {
                    errors.push(GraphqlError::at(
                        format!("fragmento desconocido `{}`", spread.name),
                        spread.position,
                    ));
                    continue;
                };
                let mut nested = permissive_selection(
                    &fragment.selection,
                    variables,
                    variable_types,
                    document,
                    depth,
                    max_depth,
                    errors,
                );
                out.fields.append(&mut nested.fields);
                out.typename_keys.append(&mut nested.typename_keys);
            }
            Selection::InlineFragment(inline) => {
                let include =
                    directives_include(&inline.directives, variables, variable_types, errors);
                if !include {
                    continue;
                }
                let mut nested = permissive_selection(
                    &inline.selection,
                    variables,
                    variable_types,
                    document,
                    depth,
                    max_depth,
                    errors,
                );
                out.fields.append(&mut nested.fields);
                out.typename_keys.append(&mut nested.typename_keys);
            }
        }
    }
    out
}
