//! Contenedor de dependencias (§6.3).
//!
//! Rust no tiene reflexión: el grafo se construye con registros tipados que la
//! macro `#[service]` emite, y se valida **al arrancar** (ADR-06).

use std::any::{Any, TypeId};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::Arc;

use crate::di::{ContainerError, FromContainer};
use crate::error::Result;

/// Un singleton ya construido, listo para guardarse en el contenedor.
pub type BoxedValue = Arc<dyn Any + Send + Sync>;

type Factory = Arc<dyn Fn(&Container) -> Result<BoxedValue> + Send + Sync + 'static>;

/// Registro de un provider: su tipo, de quién depende y cómo construirlo.
///
/// Lo emite `#[service]` (RF-021). Es clonable porque la factoría vive en un
/// `Arc`, de modo que `Module` y `App` pueden guardarlo por valor.
pub struct ProviderReg {
    /// Nombre corto del tipo provisto, para mensajes (RF-024, RF-025).
    pub type_name: String,
    /// `TypeId` provisto.
    pub provides: TypeId,
    /// Tipos que este provider necesita para construirse (RF-023).
    pub requires: Vec<TypeId>,
    /// Nombres de los tipos requeridos, para el mensaje de RF-024.
    pub requires_names: Vec<String>,
    factory: Factory,
}

impl ProviderReg {
    /// Crea el registro que emitirá `#[service]`: una factoría tipada más su lista
    /// de dependencias.
    ///
    /// La factoría recibe el contenedor **ya construido hasta ese punto del orden
    /// topológico** (RF-021), así que puede resolver sus campos por tipo sin
    /// riesgo de ver un singleton a medio hacer.
    pub fn new<T, F>(requires: Vec<(TypeId, String)>, f: F) -> Self
    where
        T: Send + Sync + 'static,
        F: Fn(&Container) -> Result<T> + Send + Sync + 'static,
    {
        let type_name = short_name::<T>();
        Self {
            type_name,
            provides: TypeId::of::<T>(),
            requires: requires.iter().map(|(id, _)| *id).collect(),
            requires_names: requires.iter().map(|(_, n)| n.clone()).collect(),
            factory: Arc::new(move |container: &Container| {
                let value = f(container)?;
                Ok(Arc::new(value) as BoxedValue)
            }),
        }
    }
}

impl fmt::Debug for ProviderReg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProviderReg")
            .field("type_name", &self.type_name)
            .field("requires", &self.requires_names)
            .finish()
    }
}

/// Contenedor de singletons, construido en orden topológico al arrancar (RF-020, RF-023).
#[derive(Clone, Default)]
pub struct Container {
    values: Arc<HashMap<TypeId, BoxedValue>>,
    names: Arc<HashMap<TypeId, String>>,
}

impl Container {
    /// Contenedor vacío, válido para una app sin servicios (RF-010).
    pub fn empty() -> Self {
        Self::default()
    }

    /// Resuelve un valor compartido por `Arc` (RF-022).
    pub fn get_arc<T: Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        self.values
            .get(&TypeId::of::<T>())
            .and_then(|v| Arc::clone(v).downcast::<T>().ok())
    }

    /// Resuelve un valor clonándolo (para services `Clone`, RF-026).
    pub fn get<T>(&self) -> Option<T>
    where
        T: Clone + Send + Sync + 'static,
    {
        self.get_arc::<T>().map(|v| (*v).clone())
    }

    /// Nombre corto registrado de un tipo, para mensajes.
    pub fn name_of(&self, id: TypeId) -> Option<&str> {
        self.names.get(&id).map(String::as_str)
    }

    /// Resuelve un valor dentro del constructor de otro provider (RF-021).
    ///
    /// Es el helper que llaman los `#[service]` generados:
    /// `c.resolve::<Repo<Task>>("TaskService", "repo")?`. Cuando la dependencia no
    /// está registrada, el error que devuelve [`FromContainer::from_container`] se
    /// reescribe con el formato completo de RF-024 (solicitante y campo).
    pub fn resolve<T>(&self, solicitante: &str, campo: &str) -> Result<T>
    where
        T: FromContainer,
    {
        T::from_container(self).map_err(|err| match err {
            crate::Error::NotFound(_) => {
                crate::Error::internal(anyhow::Error::new(ContainerError::Missing {
                    solicitante: solicitante.to_owned(),
                    campo: campo.to_owned(),
                    tipo: T::NAME.to_owned(),
                }))
            }
            other => other,
        })
    }

    /// Número de valores registrados, para `rdd check` (RF-027).
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Cierto si no hay ningún valor.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

impl fmt::Debug for Container {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Container")
            .field("values", &self.names.values().collect::<Vec<_>>())
            .finish()
    }
}

/// Constructor del contenedor: acumula providers y valores, ordena el grafo y
/// detecta ciclos (RF-023, RF-024, RF-025).
#[derive(Default)]
pub struct ContainerBuilder {
    values: HashMap<TypeId, BoxedValue>,
    names: HashMap<TypeId, String>,
    providers: Vec<Arc<ProviderReg>>,
}

impl ContainerBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registra un valor concreto (`.provide()` de RF-022).
    pub fn provide<T: Send + Sync + 'static>(&mut self, value: T) -> &mut Self {
        self.provide_value(
            TypeId::of::<T>(),
            short_name::<T>(),
            Arc::new(value) as BoxedValue,
        )
    }

    /// Registra un valor ya encajado, con su `TypeId` y nombre.
    pub fn provide_value(&mut self, id: TypeId, name: String, value: BoxedValue) -> &mut Self {
        self.names.entry(id).or_insert(name);
        self.values.insert(id, value);
        self
    }

    /// Registra el proveedor de un `#[service]` (lo emite la macro en 0.2).
    pub fn provider<T: Send + Sync + 'static>(&mut self, reg: ProviderReg) -> &mut Self {
        self.names
            .entry(TypeId::of::<T>())
            .or_insert(reg.type_name.clone());
        self.providers.push(Arc::new(reg));
        self
    }

    /// Registra un provider por referencia, sin moverlo (lo usa `App::build`).
    pub fn provider_ref(&mut self, reg: &ProviderReg) -> &mut Self {
        self.names
            .entry(reg.provides)
            .or_insert(reg.type_name.clone());
        self.providers.push(Arc::new(ProviderReg {
            type_name: reg.type_name.clone(),
            provides: reg.provides,
            requires: reg.requires.clone(),
            requires_names: reg.requires_names.clone(),
            factory: Arc::clone(&reg.factory),
        }));
        self
    }

    /// Construye el contenedor en orden topológico (RF-023) reportando el ciclo
    /// completo si existe (RF-025) o la dependencia faltante (RF-024).
    pub fn build(&self) -> std::result::Result<Container, ContainerError> {
        let mut values = self.values.clone();
        let mut names = self.names.clone();
        let mut pending: Vec<Arc<ProviderReg>> = self.providers.clone();

        while !pending.is_empty() {
            let ready = pending
                .iter()
                .position(|reg| reg.requires.iter().all(|d| values.contains_key(d)));

            let Some(index) = ready else {
                // Ningún provider avanzable: o falta una dependencia que nadie
                // provee (RF-024) o las restantes se esperan entre sí (RF-025).
                // Se comprueban todos, no solo el primero registrado.
                let faltante = pending.iter().find_map(|reg| {
                    reg.requires
                        .iter()
                        .position(|id| {
                            !values.contains_key(id) && !pending.iter().any(|p| p.provides == *id)
                        })
                        .map(|pos| (reg, pos))
                });
                return Err(match faltante {
                    Some((reg, pos)) => ContainerError::Missing {
                        solicitante: reg.type_name.clone(),
                        campo: "<constructor>".to_owned(),
                        // El nombre registrado gana; si nadie lo registró, se usa
                        // el que declaró el provider, que es el único dato que hay.
                        tipo: reg
                            .requires
                            .get(pos)
                            .and_then(|id| names.get(id).cloned())
                            .or_else(|| reg.requires_names.get(pos).cloned())
                            .unwrap_or_else(|| "<tipo sin nombre>".to_owned()),
                    },
                    None => ContainerError::Cycle {
                        ciclo: self.cycle_message(&pending),
                    },
                });
            };

            let reg = pending.remove(index);
            // La factoría ve el contenedor tal como va: todo lo ya construido, nada
            // de lo pendiente. Como el orden es topológico, sus dependencias siempre
            // están en este snapshot (RF-021).
            let partial = Container {
                values: Arc::new(values.clone()),
                names: Arc::new(names.clone()),
            };
            let value = (reg.factory)(&partial).map_err(|err| ContainerError::Init {
                tipo: reg.type_name.clone(),
                source: anyhow::Error::new(err),
            })?;
            names.entry(reg.provides).or_insert(reg.type_name.clone());
            values.insert(reg.provides, value);
        }

        Ok(Container {
            values: Arc::new(values),
            names: Arc::new(names),
        })
    }

    /// Mensaje `A -> B -> A` sobre el subgrafo bloqueado (RF-025).
    fn cycle_message(&self, stuck: &[Arc<ProviderReg>]) -> String {
        let ids: HashSet<TypeId> = stuck.iter().map(|r| r.provides).collect();
        let name = |id: &TypeId| {
            self.names
                .get(id)
                .cloned()
                .unwrap_or_else(|| "<sin nombre>".to_owned())
        };
        let mut edges: HashMap<String, Vec<String>> = HashMap::new();
        for reg in stuck {
            let from = name(&reg.provides);
            let tos: Vec<String> = reg
                .requires
                .iter()
                .filter(|d| ids.contains(d))
                .map(name)
                .collect();
            edges.entry(from).or_default().extend(tos);
        }
        let mut visited: HashMap<String, u8> = HashMap::new();
        let mut path: Vec<String> = Vec::new();
        for start in edges.keys() {
            if let Some(ciclo) = dfs_cycle(start, &edges, &mut visited, &mut path) {
                return ciclo;
            }
        }
        let nombres: Vec<String> = stuck.iter().map(|r| r.type_name.clone()).collect();
        format!(
            "dependencias insatisfechas entre: {} (RF-024/RF-025)",
            nombres.join(", ")
        )
    }
}

fn dfs_cycle(
    node: &str,
    edges: &HashMap<String, Vec<String>>,
    visited: &mut HashMap<String, u8>,
    path: &mut Vec<String>,
) -> Option<String> {
    match visited.get(node).copied().unwrap_or(0) {
        1 => {
            let start = path.iter().position(|n| n == node)?;
            let mut ciclo: Vec<&str> = path[start..].iter().map(String::as_str).collect();
            ciclo.push(node);
            return Some(ciclo.join(" -> "));
        }
        2 => return None,
        _ => {}
    }
    visited.insert(node.to_owned(), 1);
    path.push(node.to_owned());
    if let Some(nexts) = edges.get(node) {
        for n in nexts {
            if let Some(c) = dfs_cycle(n, edges, visited, path) {
                return Some(c);
            }
        }
    }
    path.pop();
    visited.insert(node.to_owned(), 2);
    None
}

/// Nombre corto de un tipo: el último segmento, sin módulos genéricos.
pub fn short_name<T: ?Sized>() -> String {
    short_from_str(std::any::type_name::<T>())
}

fn short_from_str(full: &str) -> String {
    let base = full.split('<').next().unwrap_or(full);
    base.rsplit("::").next().unwrap_or(base).to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Default)]
    struct Config(String);

    #[derive(Clone, Default)]
    struct ServiceA;

    #[derive(Clone, Default)]
    struct ServiceB;

    #[derive(Clone)]
    struct Consumer(String);

    /// Service cuyo único campo es un valor registrado (caso de `#[service]`).
    #[derive(Clone, Debug)]
    struct UsesConfig(String);

    impl FromContainer for UsesConfig {
        const NAME: &'static str = "UsesConfig";

        fn from_container(container: &Container) -> Result<Self, crate::Error> {
            container
                .get::<Config>()
                .map(|cfg| Self(cfg.0))
                .ok_or_else(|| crate::Error::not_found("no hay Config registrada"))
        }
    }

    fn provider<T>(requires: Vec<(TypeId, String)>) -> ProviderReg
    where
        T: Send + Sync + 'static + Default,
    {
        ProviderReg::new::<T, _>(requires, |_| Ok(T::default()))
    }

    #[test]
    fn resuelve_valores_registrados() {
        let mut b = ContainerBuilder::new();
        b.provide(Config("pg://x".to_owned()));
        let c = b.build().unwrap();
        assert_eq!(c.get::<Config>().unwrap().0, "pg://x");
        assert_eq!(c.len(), 1);
    }

    #[test]
    fn un_provider_es_singleton() {
        let mut b = ContainerBuilder::new();
        b.provider::<ServiceA>(provider::<ServiceA>(Vec::new()));
        let c = b.build().unwrap();
        let a1 = c.get_arc::<ServiceA>().unwrap();
        let a2 = c.get_arc::<ServiceA>().unwrap();
        assert!(
            Arc::ptr_eq(&a1, &a2),
            "el provider debe construir una sola vez"
        );
    }

    #[test]
    fn construye_en_orden_topologico() {
        let mut b = ContainerBuilder::new();
        b.provider::<ServiceB>(provider::<ServiceB>(Vec::new()));
        b.provider::<ServiceA>(provider::<ServiceA>(vec![(
            TypeId::of::<ServiceB>(),
            "ServiceB".to_owned(),
        )]));
        let c = b.build().unwrap();
        assert!(c.get::<ServiceA>().is_some());
        assert!(c.get::<ServiceB>().is_some());
    }

    #[test]
    fn dependencia_faltante_nombra_tipo_y_solicitante() {
        let mut b = ContainerBuilder::new();
        b.provider::<ServiceA>(provider::<ServiceA>(vec![(
            TypeId::of::<Config>(),
            "Config".to_owned(),
        )]));
        let err = b.build().unwrap_err();
        let msg = err.to_string();
        // RF-024: el mensaje dice qué falta, quién lo pide y cómo registrarlo.
        assert!(msg.contains("Config"), "{msg}");
        assert!(msg.contains("ServiceA"), "{msg}");
        assert!(msg.contains(".provide"), "{msg}");
    }

    #[test]
    fn ciclo_reporta_la_ruta_completa() {
        let mut b = ContainerBuilder::new();
        b.provider::<ServiceA>(provider::<ServiceA>(vec![(
            TypeId::of::<ServiceB>(),
            "ServiceB".to_owned(),
        )]));
        b.provider::<ServiceB>(provider::<ServiceB>(vec![(
            TypeId::of::<ServiceA>(),
            "ServiceA".to_owned(),
        )]));
        let err = b.build().unwrap_err();
        // RF-025: formato `A -> B -> A`.
        let msg = err.to_string();
        assert!(
            msg.contains("ServiceA -> ServiceB -> ServiceA")
                || msg.contains("ServiceB -> ServiceA -> ServiceB"),
            "{msg}"
        );
    }

    #[test]
    fn un_provider_que_falla_no_paniquea() {
        let mut b = ContainerBuilder::new();
        b.provider::<Config>(ProviderReg::new::<Config, _>(Vec::new(), |_| {
            Err(crate::Error::internal(anyhow::anyhow!("no hay url")))
        }));
        let err = b.build().unwrap_err();
        assert!(matches!(err, ContainerError::Init { .. }), "{err}");
    }

    #[test]
    fn el_provider_recibe_lo_ya_construido() {
        // RF-021: la factoría lee el contenedor para construir sus campos.
        let mut b = ContainerBuilder::new();
        b.provide(Config("pg://localhost".to_owned()));
        b.provider::<Consumer>(ProviderReg::new(
            vec![(TypeId::of::<Config>(), "Config".to_owned())],
            |c| {
                let cfg = c.get::<Config>().expect("Config va en el snapshot");
                Ok(Consumer(cfg.0.clone()))
            },
        ));
        let c = b.build().unwrap();
        assert_eq!(c.get::<Consumer>().unwrap().0, "pg://localhost");
    }

    #[test]
    fn resolver_un_dependencia_faltante_nombra_el_campo() {
        let mut b = ContainerBuilder::new();
        let c = b.build().unwrap();
        let err = c.resolve::<UsesConfig>("TaskService", "cfg").unwrap_err();
        // RF-024: solicitante + campo + tipo faltante.
        let msg = err.to_string();
        assert!(msg.contains("TaskService"), "{msg}");
        assert!(msg.contains("cfg"), "{msg}");
        assert!(msg.contains("UsesConfig"), "{msg}");
        // Sin la reescritura solo se vería el texto del `not_found` interno.
        assert!(!msg.contains("no hay Config"), "no se reescribió: {msg}");

        b.provide(Config("pg://x".to_owned()));
        let c = b.build().unwrap();
        assert_eq!(
            c.resolve::<UsesConfig>("TaskService", "cfg").unwrap().0,
            "pg://x"
        );
    }

    #[test]
    fn nombre_corto_sin_modulos_ni_genericos() {
        assert_eq!(
            short_name::<std::collections::HashMap<String, u8>>(),
            "HashMap"
        );
        assert_eq!(short_name::<Config>(), "Config");
    }
}
