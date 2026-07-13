use once_cell::sync::Lazy;
use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

pub trait Injectable: Sized + 'static {
    /// Automatically resolves dependencies from the provided container context and builds the instance
    fn resolve_new(container: &GritContainer) -> Self;
}

// A function pointer type that takes the container reference and yields an Arc trait object
pub type ComponentFactory = fn(&GritContainer) -> Arc<dyn Any + Send + Sync>;

pub struct GritContainer {
    dependencies: RwLock<HashMap<TypeId, Arc<dyn Any + Send + Sync>>>,
    factories: RwLock<HashMap<TypeId, ComponentFactory>>,
}

impl GritContainer {
    pub fn register<T: Send + Sync + 'static>(&self, dependency: T) {
        let mut cache = self.dependencies.write().unwrap();
        cache.insert(TypeId::of::<T>(), Arc::new(dependency));
    }

    pub fn register_factory<T: 'static>(&self, factory: ComponentFactory) {
        let mut facts = self.factories.write().unwrap();
        facts.insert(TypeId::of::<T>(), factory);
    }

    pub fn resolve<T: 'static + Send + Sync>(&self) -> Option<Arc<T>> {
        // Fast path: check if the instance is already initialized and cached
        {
            let cache = self.dependencies.read().unwrap();
            if let Some(any_arc) = cache.get(&TypeId::of::<T>()) {
                return any_arc.clone().downcast::<T>().ok();
            }
        }

        // Slow path: If not found, look up the factory to build it lazily on-demand
        let factory = {
            let facts = self.factories.read().unwrap();
            facts.get(&TypeId::of::<T>()).copied()
        };

        if let Some(factory_fn) = factory {
            // Execute factory to instantiate the component dynamically.
            // This will recursively trigger .resolve() on its constructor dependencies!
            let instance_any = factory_fn(self);

            // Cache it for subsequent lookups
            let mut cache = self.dependencies.write().unwrap();
            cache.insert(TypeId::of::<T>(), instance_any.clone());

            return instance_any.downcast::<T>().ok();
        }

        None
    }
}

// Global Application Context Instance
pub static CONTEXT: Lazy<GritContainer> = Lazy::new(|| GritContainer {
    dependencies: RwLock::new(HashMap::new()),
    factories: RwLock::new(HashMap::new()),
});

pub struct AutoWire;

impl AutoWire {
    /// Explicitly registers pre-constructed components (like clients, config states, or DB pools)
    pub fn component<T: Send + Sync + 'static>(component: T) {
        CONTEXT.register(component);
    }

    /// Automatically resolves all dependency constraints, instantiates the controller,
    /// and hooks it directly into the application context safely.
    pub fn controller<C: Injectable + Send + Sync + 'static>() {
        // Trigger the on-demand evaluation of the component
        let _ = CONTEXT
            .resolve::<C>()
            .expect("Failed to initialize controller via DI container");
    }
}

pub struct AutoRegisterHook {
    pub name: &'static str,
    pub register_fn: fn(&GritContainer),
}

// Allow gritshield::inventory to collect these hooks across the entire codebase
crate::inventory::collect!(AutoRegisterHook);

impl AutoWire {
    /// Phase 1: Only maps the factory recipes into the registry.
    /// No eager instantiation happens here, completely avoiding linker race conditions!
    pub fn boot_di_container() {
        for hook in crate::inventory::iter::<AutoRegisterHook> {
            (hook.register_fn)(&CONTEXT);
        }
    }
}
