//! Deserializador `serde` sobre el query string, para `Query<T>` (RF-054).
//!
//! Modelo: el query se ve como un mapa de `clave → valor`, donde una clave
//! repetida produce una secuencia. Soporta estructuras de un nivel, tipos
//! escalares, `Option<T>`, `Vec<T>` de escalares y decodificado
//! `application/x-www-form-urlencoded`.

use serde::de::{self, IntoDeserializer, MapAccess, SeqAccess, Visitor};

/// Error del deserializador de query.
#[derive(Debug)]
pub struct QueryError(String);

impl std::fmt::Display for QueryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for QueryError {}

impl de::Error for QueryError {
    fn custom<T: std::fmt::Display>(msg: T) -> Self {
        Self(msg.to_string())
    }
}

/// Separa `a=1&b=x` en pares, preservando orden y repeticiones.
pub fn parse_query(raw: &str) -> Vec<(String, String)> {
    raw.split('&')
        .filter(|s| !s.is_empty())
        .map(|pair| {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            (percent_decode(k), percent_decode(v))
        })
        .collect()
}

/// Decodificado `application/x-www-form-urlencoded` (`+` y `%XX`).
fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hex = &input[i + 1..i + 3];
                match u8::from_str_radix(hex, 16) {
                    Ok(b) => {
                        out.push(b as char);
                        i += 3;
                    }
                    Err(_) => {
                        out.push('%');
                        i += 1;
                    }
                }
            }
            b => {
                out.push(b as char);
                i += 1;
            }
        }
    }
    out
}

/// Agrupa los pares por clave, manteniendo el orden de aparición.
#[derive(Debug, Clone, Default)]
pub struct QueryPairs {
    entries: Vec<(String, Vec<String>)>,
}

impl QueryPairs {
    /// Construye el grupo a partir de los pares ya separados.
    pub fn new(pairs: &[(String, String)]) -> Self {
        let mut entries: Vec<(String, Vec<String>)> = Vec::new();
        for (k, v) in pairs {
            if let Some(e) = entries.iter_mut().find(|(ek, _)| ek == k) {
                e.1.push(v.clone());
            } else {
                entries.push((k.clone(), vec![v.clone()]));
            }
        }
        Self { entries }
    }

    /// Número de claves distintas.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Cierto si no hay ninguna clave.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl<'de> de::Deserializer<'de> for QueryPairs {
    type Error = QueryError;

    fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_map(QueryMapAccess {
            entries: self.entries,
            next: 0,
            pending: None,
        })
    }

    fn deserialize_struct<V>(
        self,
        _name: &'static str,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_any(visitor)
    }

    fn deserialize_newtype_struct<V>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_newtype_struct(self)
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf option unit unit_struct seq tuple tuple_struct map
        enum identifier ignored_any
    }
}

/// Acceso al mapa de query.
struct QueryMapAccess {
    entries: Vec<(String, Vec<String>)>,
    next: usize,
    pending: Option<Vec<String>>,
}

impl<'de> MapAccess<'de> for QueryMapAccess {
    type Error = QueryError;

    fn next_key_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>, Self::Error>
    where
        K: de::DeserializeSeed<'de>,
    {
        match self.entries.get(self.next) {
            Some((k, vals)) => {
                self.pending = Some(vals.clone());
                self.next += 1;
                seed.deserialize(k.clone().into_deserializer()).map(Some)
            }
            None => Ok(None),
        }
    }

    fn next_value_seed<V>(&mut self, seed: V) -> Result<V::Value, Self::Error>
    where
        V: de::DeserializeSeed<'de>,
    {
        let values = self.pending.take().unwrap_or_default();
        if values.len() > 1 {
            seed.deserialize(QuerySeqDeserializer { values }.into_deserializer())
        } else {
            let single = values.into_iter().next().unwrap_or_default();
            seed.deserialize(QueryStr(single).into_deserializer())
        }
    }
}

/// Deserializador de un valor de query como escalar.
#[derive(Debug, Clone)]
struct QueryStr(String);

impl<'de> IntoDeserializer<'de, QueryError> for QueryStr {
    type Deserializer = Self;
    fn into_deserializer(self) -> Self {
        self
    }
}

macro_rules! forward_str {
    ($($method:ident => $visit:ident : $ty:ty),* $(,)?) => {
        $(
            fn $method<V>(self, visitor: V) -> Result<V::Value, Self::Error>
            where
                V: Visitor<'de>,
            {
                let text = self.0;
                let parsed: $ty = if text.is_empty() {
                    <$ty>::default()
                } else {
                    text.parse::<$ty>().map_err(|_| {
                        de::Error::custom(format!("`{text}` no es un valor válido"))
                    })?
                };
                visitor.$visit(parsed)
            }
        )*
    };
}

impl<'de> de::Deserializer<'de> for QueryStr {
    type Error = QueryError;

    fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_str(&self.0)
    }

    fn deserialize_option<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        if self.0.is_empty() {
            visitor.visit_none()
        } else {
            visitor.visit_some(self)
        }
    }

    fn deserialize_enum<V>(
        self,
        _name: &str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_enum(self.0.into_deserializer())
    }

    fn deserialize_newtype_struct<V>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_newtype_struct(self)
    }

    forward_str! {
        deserialize_bool => visit_bool: bool,
        deserialize_i8 => visit_i8: i8,
        deserialize_i16 => visit_i16: i16,
        deserialize_i32 => visit_i32: i32,
        deserialize_i64 => visit_i64: i64,
        deserialize_i128 => visit_i128: i128,
        deserialize_u8 => visit_u8: u8,
        deserialize_u16 => visit_u16: u16,
        deserialize_u32 => visit_u32: u32,
        deserialize_u64 => visit_u64: u64,
        deserialize_u128 => visit_u128: u128,
        deserialize_f32 => visit_f32: f32,
        deserialize_f64 => visit_f64: f64,
    }

    serde::forward_to_deserialize_any! {
        char str string bytes byte_buf unit unit_struct seq tuple tuple_struct
        map struct identifier ignored_any
    }
}

/// Deserializador de una clave repetida, como secuencia de escalares.
struct QuerySeqDeserializer {
    values: Vec<String>,
}

impl<'de> IntoDeserializer<'de, QueryError> for QuerySeqDeserializer {
    type Deserializer = Self;
    fn into_deserializer(self) -> Self {
        self
    }
}

impl<'de> de::Deserializer<'de> for QuerySeqDeserializer {
    type Error = QueryError;

    fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_seq(QuerySeqAccess {
            values: self.values,
            next: 0,
        })
    }

    fn deserialize_seq<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_any(visitor)
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf option newtype_struct unit unit_struct tuple tuple_struct
        map struct enum identifier ignored_any
    }
}

struct QuerySeqAccess {
    values: Vec<String>,
    next: usize,
}

impl<'de> SeqAccess<'de> for QuerySeqAccess {
    type Error = QueryError;

    fn next_element_seed<T>(&mut self, seed: T) -> Result<Option<T::Value>, Self::Error>
    where
        T: de::DeserializeSeed<'de>,
    {
        match self.values.get(self.next) {
            Some(v) => {
                let v = v.clone();
                self.next += 1;
                seed.deserialize(QueryStr(v).into_deserializer()).map(Some)
            }
            None => Ok(None),
        }
    }
}
