//! `Module` y `Controller` (§6.2, §3). Un módulo es una carpeta que agrupa un
//! contexto y se registra con `.module(...)`.

use std::sync::Arc;

use crate::api::Router;
use crate::di::ProviderReg;
use crate::http::routes::RouteDef;
use crate::http::types::Middleware;

/// Adaptador HTTP que traduce petición a llamada de servicio y resultado a
/// respuesta. No decide nada (§3, RA-001).
///
/// Lo implementa la macro `#[controller]` (RF-056); el usuario no lo escribe a
/// mano salvo que registre rutas de forma explícita.
pub trait Controller: Send + Sync + 'static {
    /// Nombre del controller, para logs y diagnósticos.
    const NAME: &'static str;

    /// Registra las rutas del controller en el acumulador.
    fn register(router: &mut Router);

    /// Middlewares declarados a nivel de controller con `#[middleware]` (§11.4).
    fn middlewares() -> Vec<Arc<dyn Middleware>> {
        Vec::new()
    }

    /// Las rutas ya materializadas, con los middlewares de controller compuestos
    /// por fuera de los de cada ruta (RF-056).
    ///
    /// La implementación por defecto monta el controller en un [`Router`]; la
    /// macro no lo reemplaza, solo alimenta `register`.
    fn routes() -> Vec<RouteDef> {
        let mut router = Router::new(Self::NAME);
        Self::register(&mut router);
        let middlewares = Self::middlewares();
        router
            .into_defs()
            .into_iter()
            .map(|def| def.wrapped_with(&middlewares))
            .collect()
    }
}

/// Un módulo: providers, controllers y middlewares propios (§6.2).
#[derive(Default)]
pub struct Module {
    pub(crate) name: &'static str,
    pub(crate) controllers: Vec<Arc<dyn ControllerDyn>>,
    pub(crate) middlewares: Vec<Arc<dyn Middleware>>,
    pub(crate) providers: Vec<ProviderReg>,
}

impl Module {
    /// Crea un módulo vacío con nombre, para construirlo por API (§6.2).
    pub fn new(name: &'static str) -> Self {
        Self {
            name,
            ..Default::default()
        }
    }

    /// Registra un controller (lo emite `module!`).
    pub fn controller<C: Controller>(mut self) -> Self {
        self.controllers
            .push(Arc::new(ControllerAdapter::<C>::new()));
        self
    }

    /// Añade un middleware que aplica solo a este módulo (§11.3).
    pub fn middleware<M: Middleware>(mut self, mw: M) -> Self {
        self.middlewares.push(Arc::new(mw));
        self
    }

    /// Añade un middleware ya encajado.
    pub fn boxed_middleware(mut self, mw: Arc<dyn Middleware>) -> Self {
        self.middlewares.push(mw);
        self
    }

    /// Añade un provider (lo emite `#[service]` en 0.2).
    pub fn provider(mut self, reg: ProviderReg) -> Self {
        self.providers.push(reg);
        self
    }

    /// Nombre del módulo, para el registro de arranque (RN-042).
    pub fn name(&self) -> &'static str {
        self.name
    }
}

/// Versión objet-safe de [`Controller`], para guardarla en el módulo.
pub(crate) trait ControllerDyn: Send + Sync + 'static {
    /// Rutas del controller con los middlewares de módulo y de controller
    /// ya compuestos, en el orden de §11.3.
    fn defs(&self, module_middlewares: &[Arc<dyn Middleware>]) -> Vec<RouteDef>;
}

struct ControllerAdapter<C> {
    marker: std::marker::PhantomData<C>,
}

impl<C> ControllerAdapter<C> {
    fn new() -> Self {
        Self {
            marker: std::marker::PhantomData,
        }
    }
}

impl<C: Controller> ControllerDyn for ControllerAdapter<C> {
    fn defs(&self, module_middlewares: &[Arc<dyn Middleware>]) -> Vec<RouteDef> {
        // `Controller::routes()` ya trae el controller envuelto por sus
        // middlewares; el módulo queda un nivel más afuera (RF-080).
        C::routes()
            .into_iter()
            .map(|def| def.wrapped_with(module_middlewares))
            .collect()
    }
}

/// Constructor del `module!` (§6.2): acumula tipos clasificados por la macro.
#[derive(Default)]
pub struct ModuleBuilder {
    name: &'static str,
    controllers: Vec<Arc<dyn ControllerDyn>>,
    middlewares: Vec<Arc<dyn Middleware>>,
    providers: Vec<ProviderReg>,
}

impl ModuleBuilder {
    /// Módulo con nombre explícito.
    pub fn named(name: &'static str) -> Self {
        Self {
            name,
            ..Default::default()
        }
    }

    /// Declara un controller del módulo.
    pub fn controller<C: Controller>(self) -> Self {
        self.with_controller(Arc::new(ControllerAdapter::<C>::new()))
    }

    /// Declara un miembro que sabe registrarse solo: es la forma abreviada de
    /// `module!(TaskService, TaskController)` (§6.2).
    pub fn member<T: ModuleMember>(self) -> Self {
        T::register(self)
    }

    /// Añade un middleware de módulo.
    pub fn middleware<M: Middleware>(mut self, mw: M) -> Self {
        self.middlewares.push(Arc::new(mw));
        self
    }

    /// Añade un provider ya construido.
    pub fn provide(mut self, reg: ProviderReg) -> Self {
        self.providers.push(reg);
        self
    }

    /// Punto de entrada interno: la macro emite `.controller::<C>()`, que sí
    /// puede nombrar el tipo.
    pub(crate) fn with_controller(self, c: Arc<dyn ControllerDyn>) -> Self {
        let mut this = self;
        this.controllers.push(c);
        this
    }

    /// Nombre legible del módulo, derivado del primer controller si no se fija.
    pub fn with_name(mut self, name: &'static str) -> Self {
        self.name = name;
        self
    }

    /// Construye el módulo.
    pub fn build(self) -> Module {
        let name = if self.name.is_empty() {
            "module"
        } else {
            self.name
        };
        Module {
            name,
            controllers: self.controllers,
            middlewares: self.middlewares,
            providers: self.providers,
        }
    }
}

/// Marca que implementa todo `#[service]`: expone su registro de contenedor.
pub trait ProviderMarker {
    /// Registro del provider, con sus dependencias (RF-021, RF-023).
    fn reg() -> ProviderReg;
}

/// Miembro de un módulo: `#[service]` y `#[controller]` generan esta impl, que es
/// lo que permite la forma abreviada `module!(TaskService, TaskController)` (§6.2).
///
/// Una macro no puede clasificar tipos, así que el conocimiento de «cómo me
/// registro» vive en el propio tipo; por eso el trait se implementa explícitamente
/// y no con un blanket sobre `Controller`.
pub trait ModuleMember: Send + Sync + 'static {
    /// Añade este miembro al constructor del módulo.
    fn register(builder: ModuleBuilder) -> ModuleBuilder;
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::pin::Pin;

    use bytes::Bytes;

    use super::*;
    use crate::api::route_def;
    use crate::app::App;
    use crate::http::types::{Method, NextArc, Request, Response, StatusCode};

    /// Middleware de prueba: anota su nombre en la respuesta al salir.
    struct Mark(&'static str);

    impl Middleware for Mark {
        fn handle<'a>(
            &'a self,
            req: Request,
            next: NextArc,
        ) -> Pin<Box<dyn Future<Output = crate::Result<Response>> + Send + 'a>> {
            Box::pin(async move {
                let mut resp = next(req).await?;
                resp.headers_mut()
                    .append("x-order", http::HeaderValue::from_static(self.0));
                Ok(resp)
            })
        }
    }

    /// Controller escrito a mano, con un middleware por ruta y otro por controller.
    struct C;

    impl Controller for C {
        const NAME: &'static str = "C";

        fn register(router: &mut Router) {
            router.push(route_def(
                Method::GET,
                "/x",
                StatusCode::NO_CONTENT,
                vec![Arc::new(Mark("ruta"))],
                |_ctx| async move { Ok(()) },
            ));
        }

        fn middlewares() -> Vec<Arc<dyn Middleware>> {
            vec![Arc::new(Mark("controller"))]
        }
    }

    fn get(uri: &str) -> Request {
        http::Request::builder()
            .method(Method::GET)
            .uri(uri)
            .body(Bytes::new())
            .unwrap()
    }

    #[tokio::test]
    async fn el_orden_es_global_modulo_controller_ruta() {
        // RF-080: el primero registrado es el más externo; al salir se anotan en
        // orden inverso, así que la cabecera delata cómo quedó anidada la cadena.
        let module = Module::new("m")
            .boxed_middleware(Arc::new(Mark("modulo")))
            .controller::<C>();
        let built = App::new()
            .middleware(Mark("global"))
            .module(module)
            .with_health(false)
            .build()
            .unwrap();

        let resp = built.http.dispatch(get("/x")).await;
        let order: Vec<String> = resp
            .headers()
            .get_all("x-order")
            .iter()
            .map(|v| v.to_str().unwrap().to_owned())
            .collect();
        assert_eq!(
            order,
            vec!["ruta", "controller", "modulo", "global"],
            "la cadena quedó anidada al revés"
        );
    }

    #[tokio::test]
    async fn routes_devuelve_rutas_materializadas_sin_globales() {
        // RF-056: `routes()` es público y ya trae lo que aporta el controller.
        let defs = C::routes();
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].path, "/x");

        let resp = defs[0].handler.as_ref()(get("/x")).await;
        let order: Vec<String> = resp
            .headers()
            .get_all("x-order")
            .iter()
            .map(|v| v.to_str().unwrap().to_owned())
            .collect();
        assert_eq!(order, vec!["ruta", "controller"]);
    }
}
