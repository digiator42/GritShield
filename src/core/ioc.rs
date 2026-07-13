use colored::*;
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

/// Emitted once per registered component (via #[component] or #[derive(GritComponent)],
/// or manually through `provide!`). Purely metadata — carries no construction logic.
pub struct ProvidedComponent {
    pub name: &'static str,
}

crate::inventory::collect!(ProvidedComponent);

/// Emitted once per resolved dependency, wherever `CONTEXT.resolve::<T>()` is generated:
/// component constructors, GritComponent struct fields, controller methods, and
/// standalone route handler arguments. `component` is the thing that needs `requires`.
pub struct DependencyEdge {
    pub component: &'static str,
    pub requires: &'static str,
}
crate::inventory::collect!(DependencyEdge);

impl AutoWire {
    /// Walks every collected dependency edge and checks it against every collected
    /// provider. Returns every missing dependency at once (not just the first one
    /// hit at runtime).
    pub fn verify() -> Result<(), Vec<String>> {
        let provided: std::collections::HashSet<&str> =
            crate::inventory::iter::<ProvidedComponent>()
                .into_iter()
                .map(|p| p.name)
                .collect();

        let missing: Vec<String> = crate::inventory::iter::<DependencyEdge>()
            .into_iter()
            .filter(|edge| !provided.contains(edge.requires))
            .map(|edge| {
                format!(
                    "  - '{}' requires '{}', but nothing registered that type. \
             Add #[component] / #[derive(GritComponent)] to it, or register it \
             explicitly with provide!({}, ...).",
                    edge.component.green(),
                    edge.requires.red(),  
                    edge.requires.yellow(),
                )
            })
            .collect();

        if missing.is_empty() {
            Ok(())
        } else {
            Err(missing)
        }
    }

    /// Now fails fast: the entire DI graph is checked for completeness *before* any
    /// factory runs, so a missing dependency surfaces once, at boot.
    pub fn boot_di_container() {
        if let Err(errors) = Self::verify() {
            panic!(
                "{} ({} missing dependenc{}):\n{}",
                "GritShield DI graph is incomplete".bold().red(),
                errors.len().to_string().bold().red(),
                if errors.len() == 1 { "y" } else { "ies" },
                errors.join("\n")
            );
        }

        for hook in crate::inventory::iter::<AutoRegisterHook> {
            (hook.register_fn)(&CONTEXT);
        }
    }
}

/// Registers a component that has no `#[component]`/`#[derive(GritComponent)]` of its
/// own — typically a raw config value (an API key string, a connection URL, a numeric
/// limit) — while still emitting the `ProvidedComponent` metadata that `AutoWire::verify()`
/// needs to see it. Use this in place of a bare `AutoWire::component(...)` call so a
/// forgotten registration shows up as a graph-verification failure instead of a runtime
/// `.expect()` panic the first time something tries to resolve it.
///
/// ```ignore
/// provide!(StripeApiKey, StripeApiKey("sk_live_...".to_string()));
/// ```
#[macro_export]
macro_rules! provide {
    ($ty:ty, $value:expr) => {
        $crate::inventory::submit! {
            $crate::core::ioc::ProvidedComponent { name: std::stringify!($ty) }
        }
        $crate::core::ioc::AutoWire::component::<$ty>($value);
    };
}
