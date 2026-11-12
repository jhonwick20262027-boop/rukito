//! Lexer y parser del subconjunto de documento de §10.9 (`RF-235`).
//!
//! No hay SDL: el documento es una consulta, mutación o suscripción. Cualquier
//! construcción fuera del subconjunto se rechaza con un error con posición para
//! que el cliente reciba `locations`.

use std::collections::HashMap;

/// Posición 1-based en el documento.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Position {
    /// Línea.
    pub line: u32,
    /// Columna.
    pub column: u32,
}

/// Error de sintaxis con su posición.
#[derive(Debug)]
pub struct ParseError {
    /// Mensaje para el cliente.
    pub message: String,
    /// Dónde ocurrió.
    pub position: Position,
}

impl ParseError {
    fn new(message: impl Into<String>, position: Position) -> Self {
        Self {
            message: message.into(),
            position,
        }
    }
}

/// Documento ya parseado.
#[derive(Debug, Default)]
pub struct Document {
    /// Operaciones en orden.
    pub operations: Vec<Operation>,
    /// Fragmentos por nombre.
    pub fragments: HashMap<String, Fragment>,
    /// Posición de cada fragmento, para diagnosticar duplicados.
    pub fragment_positions: HashMap<String, Position>,
}

/// Tipo de operación.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OperationKind {
    /// `query`.
    Query,
    /// `mutation`.
    Mutation,
    /// `subscription`.
    Subscription,
}

impl OperationKind {
    /// `query`, `mutation` o `subscription`.
    pub fn keyword(self) -> &'static str {
        match self {
            Self::Query => "query",
            Self::Mutation => "mutation",
            Self::Subscription => "subscription",
        }
    }

    /// Tipo raíz que usa el esquema.
    pub fn root_kind(self) -> crate::RootKind {
        match self {
            Self::Query => crate::RootKind::Query,
            Self::Mutation => crate::RootKind::Mutation,
            Self::Subscription => crate::RootKind::Subscription,
        }
    }
}

/// Operación del documento.
#[derive(Debug)]
pub struct Operation {
    /// Tipo.
    pub kind: OperationKind,
    /// Nombre opcional.
    pub name: Option<String>,
    /// Variables declaradas.
    pub variables: Vec<VariableDef>,
    /// Selección raíz.
    pub selection: Vec<Selection>,
    /// Inicio de la operación.
    pub position: Position,
}

/// Declaración de variable.
#[derive(Debug)]
pub struct VariableDef {
    /// Nombre sin `$`.
    pub name: String,
    /// Tipo declarado.
    pub ty: crate::TypeRef,
    /// Valor por defecto.
    pub default: Option<Value>,
    /// Posición de la declaración.
    pub position: Position,
}

/// Fragmento con nombre.
#[derive(Debug)]
pub struct Fragment {
    /// Condición de tipo.
    pub type_condition: String,
    /// Directivas.
    pub directives: Vec<Directive>,
    /// Selección.
    pub selection: Vec<Selection>,
    /// Posición.
    pub position: Position,
}

/// Una selección.
#[derive(Debug)]
pub enum Selection {
    /// Campo.
    Field(Field),
    /// `...Nombre`.
    FragmentSpread(FragmentSpread),
    /// `... on Tipo { … }`.
    InlineFragment(InlineFragment),
}

/// Campo de una selección.
#[derive(Debug)]
pub struct Field {
    /// Alias, si lo hay.
    pub alias: Option<String>,
    /// Nombre en el esquema.
    pub name: String,
    /// Argumentos.
    pub arguments: Vec<Argument>,
    /// Directivas.
    pub directives: Vec<Directive>,
    /// Subselección.
    pub selection: Vec<Selection>,
    /// Posición del nombre.
    pub position: Position,
}

/// Uso de un fragmento.
#[derive(Debug)]
pub struct FragmentSpread {
    /// Nombre.
    pub name: String,
    /// Directivas.
    pub directives: Vec<Directive>,
    /// Posición.
    pub position: Position,
}

/// Fragmento inline.
#[derive(Debug)]
pub struct InlineFragment {
    /// Condición de tipo, si la hay.
    pub type_condition: Option<String>,
    /// Directivas.
    pub directives: Vec<Directive>,
    /// Selección.
    pub selection: Vec<Selection>,
    /// Posición.
    pub position: Position,
}

/// Argumento con su posición.
#[derive(Debug)]
pub struct Argument {
    /// Nombre.
    pub name: String,
    /// Valor, posiblemente una variable.
    pub value: Value,
    /// Posición.
    pub position: Position,
}

/// Directiva.
#[derive(Debug)]
pub struct Directive {
    /// Nombre.
    pub name: String,
    /// Argumentos.
    pub arguments: Vec<Argument>,
    /// Posición.
    pub position: Position,
}

/// Valor literal del documento.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// `null`.
    Null,
    /// Booleano.
    Boolean(bool),
    /// Entero.
    Integer(i64),
    /// Flotante.
    Float(f64),
    /// Cadena.
    String(String),
    /// `$variable`.
    Variable(String),
    /// Lista.
    List(Vec<Value>),
    /// Objeto.
    Object(Vec<(String, Value)>),
    /// Nombre que no es `true`, `false` ni `null` (enum): se rechaza al validar.
    Enum(String),
}

impl Value {
    /// Nombre de la variable si el valor es una.
    pub fn variable(&self) -> Option<&str> {
        match self {
            Self::Variable(name) => Some(name),
            _ => None,
        }
    }
}

/// Parsea un documento completo.
pub fn parse(source: &str) -> Result<Document, ParseError> {
    let tokens = lex(source)?;
    let mut parser = Parser { tokens, index: 0 };
    parser.document()
}

#[derive(Clone, Debug)]
enum Token {
    Name(String),
    Int(i64),
    Float(f64),
    Str(String),
    Punct(char),
    Spread,
    Eof,
}

#[derive(Clone, Debug)]
struct Spanned {
    token: Token,
    position: Position,
}

fn lex(source: &str) -> Result<Vec<Spanned>, ParseError> {
    let chars: Vec<char> = source.chars().collect();
    let mut tokens = Vec::new();
    let mut index = 0;
    let mut line = 1;
    let mut column = 1;

    let advance = |index: &mut usize, line: &mut u32, column: &mut u32, ch: char| {
        *index += 1;
        if ch == '\n' {
            *line += 1;
            *column = 1;
        } else {
            *column += 1;
        }
    };

    while index < chars.len() {
        let ch = chars[index];
        let position = Position { line, column };
        match ch {
            ' ' | '\t' | '\r' | ',' => advance(&mut index, &mut line, &mut column, ch),
            '\n' => advance(&mut index, &mut line, &mut column, ch),
            '#' => {
                while index < chars.len() && chars[index] != '\n' {
                    let ch = chars[index];
                    advance(&mut index, &mut line, &mut column, ch);
                }
            }
            '!' | '$' | '&' | '(' | ')' | ':' | '=' | '@' | '[' | ']' | '{' | '|' | '}' => {
                tokens.push(Spanned {
                    token: Token::Punct(ch),
                    position,
                });
                advance(&mut index, &mut line, &mut column, ch);
            }
            '.' => {
                if index + 2 < chars.len() && chars[index + 1] == '.' && chars[index + 2] == '.' {
                    tokens.push(Spanned {
                        token: Token::Spread,
                        position,
                    });
                    for _ in 0..3 {
                        let ch = chars[index];
                        advance(&mut index, &mut line, &mut column, ch);
                    }
                } else {
                    return Err(ParseError::new(
                        "se esperaba `...` para un fragmento",
                        position,
                    ));
                }
            }
            '"' => {
                let (value, next_index, next_line, next_column) =
                    lex_string(&chars, index, line, column, position)?;
                tokens.push(Spanned {
                    token: Token::Str(value),
                    position,
                });
                index = next_index;
                line = next_line;
                column = next_column;
            }
            '-' | '0'..='9' => {
                let start = index;
                if ch == '-' {
                    advance(&mut index, &mut line, &mut column, ch);
                }
                while index < chars.len() && chars[index].is_ascii_digit() {
                    let ch = chars[index];
                    advance(&mut index, &mut line, &mut column, ch);
                }
                let mut float = false;
                if index < chars.len() && chars[index] == '.' {
                    float = true;
                    let ch = chars[index];
                    advance(&mut index, &mut line, &mut column, ch);
                    while index < chars.len() && chars[index].is_ascii_digit() {
                        let ch = chars[index];
                        advance(&mut index, &mut line, &mut column, ch);
                    }
                }
                if index < chars.len() && (chars[index] == 'e' || chars[index] == 'E') {
                    float = true;
                    let ch = chars[index];
                    advance(&mut index, &mut line, &mut column, ch);
                    if index < chars.len() && (chars[index] == '+' || chars[index] == '-') {
                        let ch = chars[index];
                        advance(&mut index, &mut line, &mut column, ch);
                    }
                    while index < chars.len() && chars[index].is_ascii_digit() {
                        let ch = chars[index];
                        advance(&mut index, &mut line, &mut column, ch);
                    }
                }
                let text: String = chars[start..index].iter().collect();
                let token = if float {
                    text.parse::<f64>()
                        .map(Token::Float)
                        .map_err(|_| ParseError::new("número inválido", position))?
                } else {
                    text.parse::<i64>()
                        .map(Token::Int)
                        .map_err(|_| ParseError::new("entero fuera de rango", position))?
                };
                tokens.push(Spanned { token, position });
            }
            '_' | 'a'..='z' | 'A'..='Z' => {
                let start = index;
                while index < chars.len()
                    && (chars[index].is_ascii_alphanumeric() || chars[index] == '_')
                {
                    let ch = chars[index];
                    advance(&mut index, &mut line, &mut column, ch);
                }
                tokens.push(Spanned {
                    token: Token::Name(chars[start..index].iter().collect()),
                    position,
                });
            }
            other => {
                return Err(ParseError::new(
                    format!("carácter inesperado `{other}`"),
                    position,
                ));
            }
        }
    }

    tokens.push(Spanned {
        token: Token::Eof,
        position: Position { line, column },
    });
    Ok(tokens)
}

/// Lee una cadena y devuelve el valor, el índice siguiente y la posición.
fn lex_string(
    chars: &[char],
    start: usize,
    line: u32,
    column: u32,
    position: Position,
) -> Result<(String, usize, u32, u32), ParseError> {
    let mut index = start + 1;
    let mut column = column + 1;
    let mut out = String::new();
    while index < chars.len() {
        let ch = chars[index];
        if ch == '"' {
            return Ok((out, index + 1, line, column + 1));
        }
        if ch == '\\' {
            index += 1;
            column += 1;
            let Some(&escape) = chars.get(index) else {
                return Err(ParseError::new("cadena sin cerrar", position));
            };
            if escape == 'u' {
                let mut code = 0u32;
                for offset in 1..=4 {
                    let digit = chars
                        .get(index + offset)
                        .and_then(|ch| ch.to_digit(16))
                        .ok_or_else(|| ParseError::new("escape `\\u` inválido", position))?;
                    code = code * 16 + digit;
                }
                let decoded = char::from_u32(code)
                    .ok_or_else(|| ParseError::new("escape `\\u` inválido", position))?;
                out.push(decoded);
                index += 5;
                column += 5;
                continue;
            }
            let decoded = match escape {
                '"' => '"',
                '\\' => '\\',
                '/' => '/',
                'b' => '\u{0008}',
                'f' => '\u{000c}',
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                other => {
                    return Err(ParseError::new(
                        format!("escape desconocido `\\{other}`"),
                        position,
                    ));
                }
            };
            out.push(decoded);
            index += 1;
            column += 1;
            continue;
        }
        if ch == '\n' {
            return Err(ParseError::new(
                "la cadena no puede cruzar líneas",
                position,
            ));
        }
        out.push(ch);
        index += 1;
        column += 1;
    }
    Err(ParseError::new("cadena sin cerrar", position))
}

struct Parser {
    tokens: Vec<Spanned>,
    index: usize,
}

impl Parser {
    fn peek(&self) -> &Token {
        &self.tokens[self.index].token
    }

    fn position(&self) -> Position {
        self.tokens[self.index].position
    }

    fn bump(&mut self) -> Token {
        let token = self.tokens[self.index].token.clone();
        if self.index + 1 < self.tokens.len() {
            self.index += 1;
        }
        token
    }

    fn eat_punct(&mut self, punct: char) -> bool {
        if matches!(self.peek(), Token::Punct(ch) if *ch == punct) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn expect_punct(&mut self, punct: char) -> Result<(), ParseError> {
        if self.eat_punct(punct) {
            Ok(())
        } else {
            Err(ParseError::new(
                format!("se esperaba `{punct}`"),
                self.position(),
            ))
        }
    }

    fn eat_name(&mut self, expected: &str) -> bool {
        if matches!(self.peek(), Token::Name(name) if name == expected) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn take_name(&mut self) -> Result<String, ParseError> {
        match self.peek().clone() {
            Token::Name(name) => {
                self.bump();
                Ok(name)
            }
            _ => Err(ParseError::new("se esperaba un nombre", self.position())),
        }
    }

    fn document(&mut self) -> Result<Document, ParseError> {
        let mut document = Document::default();
        while !matches!(self.peek(), Token::Eof) {
            match self.peek() {
                Token::Punct('{') => {
                    document.operations.push(self.operation(None)?);
                }
                Token::Name(keyword)
                    if keyword == "query" || keyword == "mutation" || keyword == "subscription" =>
                {
                    let kind = match keyword.as_str() {
                        "query" => OperationKind::Query,
                        "mutation" => OperationKind::Mutation,
                        _ => OperationKind::Subscription,
                    };
                    self.bump();
                    document.operations.push(self.operation(Some(kind))?);
                }
                Token::Name(keyword) if keyword == "fragment" => {
                    self.bump();
                    let name = self.take_name()?;
                    let position = self.position();
                    if !self.eat_name("on") {
                        return Err(ParseError::new("un fragmento necesita `on Tipo`", position));
                    }
                    let type_condition = self.take_name()?;
                    let directives = self.directives()?;
                    let selection = self.selection_set()?;
                    if document.fragments.contains_key(&name) {
                        return Err(ParseError::new(
                            format!("fragmento duplicado `{name}`"),
                            position,
                        ));
                    }
                    document.fragment_positions.insert(name.clone(), position);
                    document.fragments.insert(
                        name,
                        Fragment {
                            type_condition,
                            directives,
                            selection,
                            position,
                        },
                    );
                }
                Token::Name(other) => {
                    return Err(ParseError::new(
                        format!("se esperaba una operación, no `{other}`"),
                        self.position(),
                    ));
                }
                _ => {
                    return Err(ParseError::new(
                        "se esperaba una operación o un fragmento",
                        self.position(),
                    ));
                }
            }
        }
        if document.operations.is_empty() {
            return Err(ParseError::new(
                "el documento no contiene operaciones",
                self.position(),
            ));
        }
        Ok(document)
    }

    fn operation(&mut self, kind: Option<OperationKind>) -> Result<Operation, ParseError> {
        let position = self.position();
        let kind = kind.unwrap_or(OperationKind::Query);
        let name = match self.peek() {
            Token::Name(_) => Some(self.take_name()?),
            _ => None,
        };
        let variables = if self.eat_punct('(') {
            let mut variables = Vec::new();
            while !self.eat_punct(')') {
                variables.push(self.variable()?);
            }
            variables
        } else {
            Vec::new()
        };
        // Directivas de operación: se aceptan y se validan como las de campo.
        let _directives = self.directives()?;
        let selection = self.selection_set()?;
        Ok(Operation {
            kind,
            name,
            variables,
            selection,
            position,
        })
    }

    fn variable(&mut self) -> Result<VariableDef, ParseError> {
        let position = self.position();
        self.expect_punct('$')?;
        let name = self.take_name()?;
        self.expect_punct(':')?;
        let ty = self.type_ref()?;
        let default = if self.eat_punct('=') {
            Some(self.value(true)?)
        } else {
            None
        };
        Ok(VariableDef {
            name,
            ty,
            default,
            position,
        })
    }

    fn type_ref(&mut self) -> Result<crate::TypeRef, ParseError> {
        let mut ty = if self.eat_punct('[') {
            let inner = self.type_ref()?;
            self.expect_punct(']')?;
            crate::TypeRef::List(Box::new(inner))
        } else {
            crate::TypeRef::Named(self.take_name()?)
        };
        if self.eat_punct('!') {
            ty = ty.non_null();
        }
        Ok(ty)
    }

    fn selection_set(&mut self) -> Result<Vec<Selection>, ParseError> {
        self.expect_punct('{')?;
        let mut selection = Vec::new();
        while !self.eat_punct('}') {
            if matches!(self.peek(), Token::Eof) {
                return Err(ParseError::new("selección sin cerrar", self.position()));
            }
            selection.push(self.selection()?);
        }
        if selection.is_empty() {
            return Err(ParseError::new(
                "una selección no puede estar vacía",
                self.position(),
            ));
        }
        Ok(selection)
    }

    fn selection(&mut self) -> Result<Selection, ParseError> {
        if matches!(self.peek(), Token::Spread) {
            let position = self.position();
            self.bump();
            if self.eat_name("on") {
                let type_condition = Some(self.take_name()?);
                let directives = self.directives()?;
                let selection = self.selection_set()?;
                return Ok(Selection::InlineFragment(InlineFragment {
                    type_condition,
                    directives,
                    selection,
                    position,
                }));
            }
            if matches!(self.peek(), Token::Punct('{') | Token::Punct('@')) {
                let directives = self.directives()?;
                let selection = self.selection_set()?;
                return Ok(Selection::InlineFragment(InlineFragment {
                    type_condition: None,
                    directives,
                    selection,
                    position,
                }));
            }
            let name = self.take_name()?;
            let directives = self.directives()?;
            return Ok(Selection::FragmentSpread(FragmentSpread {
                name,
                directives,
                position,
            }));
        }

        let position = self.position();
        let name = self.take_name()?;
        let alias = if self.eat_punct(':') {
            Some(name.clone())
        } else {
            None
        };
        let name = if alias.is_some() {
            self.take_name()?
        } else {
            name
        };
        let arguments = self.arguments()?;
        let directives = self.directives()?;
        let selection = if matches!(self.peek(), Token::Punct('{')) {
            self.selection_set()?
        } else {
            Vec::new()
        };
        Ok(Selection::Field(Field {
            alias,
            name,
            arguments,
            directives,
            selection,
            position,
        }))
    }

    fn arguments(&mut self) -> Result<Vec<Argument>, ParseError> {
        if !self.eat_punct('(') {
            return Ok(Vec::new());
        }
        let mut arguments = Vec::new();
        while !self.eat_punct(')') {
            let position = self.position();
            let name = self.take_name()?;
            self.expect_punct(':')?;
            let value = self.value(false)?;
            arguments.push(Argument {
                name,
                value,
                position,
            });
        }
        Ok(arguments)
    }

    fn directives(&mut self) -> Result<Vec<Directive>, ParseError> {
        let mut directives = Vec::new();
        while self.eat_punct('@') {
            let position = self.position();
            let name = self.take_name()?;
            let arguments = self.arguments()?;
            directives.push(Directive {
                name,
                arguments,
                position,
            });
        }
        Ok(directives)
    }

    fn value(&mut self, constant: bool) -> Result<Value, ParseError> {
        match self.peek().clone() {
            Token::Punct('$') => {
                if constant {
                    return Err(ParseError::new(
                        "el valor por defecto no puede usar variables",
                        self.position(),
                    ));
                }
                self.bump();
                Ok(Value::Variable(self.take_name()?))
            }
            Token::Int(number) => {
                self.bump();
                Ok(Value::Integer(number))
            }
            Token::Float(number) => {
                self.bump();
                Ok(Value::Float(number))
            }
            Token::Str(text) => {
                self.bump();
                Ok(Value::String(text))
            }
            Token::Name(name) => {
                self.bump();
                Ok(match name.as_str() {
                    "true" => Value::Boolean(true),
                    "false" => Value::Boolean(false),
                    "null" => Value::Null,
                    _ => Value::Enum(name),
                })
            }
            Token::Punct('[') => {
                self.bump();
                let mut items = Vec::new();
                while !self.eat_punct(']') {
                    items.push(self.value(constant)?);
                }
                Ok(Value::List(items))
            }
            Token::Punct('{') => {
                self.bump();
                let mut entries = Vec::new();
                while !self.eat_punct('}') {
                    let name = self.take_name()?;
                    self.expect_punct(':')?;
                    entries.push((name, self.value(constant)?));
                }
                Ok(Value::Object(entries))
            }
            _ => Err(ParseError::new("se esperaba un valor", self.position())),
        }
    }
}
