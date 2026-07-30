use colored::*;
use once_cell::sync::Lazy;
use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::fmt::Write;

pub trait Injectable: Sized + 'static {
    /// Automatically resolves dependencies from the provided container context and builds the instance
    fn resolve_new(container: &GritContainer) -> Self;
}

/// Marker trait indicating a type can be implicitly pulled out of the global dynamic `CONTEXT`.
pub trait RuntimeInjectable {}

// A function pointer type that takes the container reference and yields an Arc trait object
pub type ComponentFactory = fn(&GritContainer) -> Arc<dyn Any + Send + Sync>;

/// A compile-time proof that a container holds a specific dependency `T`.
pub trait HasComponent<T> {
    fn get_component(&self) -> std::sync::Arc<T>;
}

/// A marker trait for strict compile-time App Containers
pub trait StrictContainer: Sized + Send + Sync + 'static {}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComponentKind {
    Controller,
    Transient,
    Singleton,
    Primitive,
}

/// Emitted once per registered component (via #[component] or #[derive(GritComponent)],
/// or manually through `provide!`). Purely metadata — carries no construction logic.
pub struct ProvidedComponent {
    pub name: &'static str,
    pub kind: ComponentKind,
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
             explicitly with inject!({}, ...).",
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

    /// Generates a Mermaid markdown diagram string showing component dependency paths.
    pub fn export_mermaid() -> String {
        let mut graph = String::from("```mermaid\ngraph TD\n");

        // Render all registered components as nodes
        for component in crate::inventory::iter::<ProvidedComponent> {
            let _ = writeln!(graph, "    {}[[\"{}\"]]", component.name, component.name);
        }

        graph.push_str("\n");

        // Render dependency edges (Component -> Requires)
        for edge in crate::inventory::iter::<DependencyEdge> {
            let _ = writeln!(
                graph,
                "    {} -->|\"requires\"| {}",
                edge.component, edge.requires
            );
        }

        graph.push_str("```\n");
        graph
    }

    /// Generates a Graphviz `.dot` file representation.
    pub fn export_dot() -> String {
        let mut dot = String::from(
            "digraph SystemTopology {\n\
            \trankdir=LR;\n\
            \tbgcolor=\"transparent\";\n\
            \tnode [style=\"filled,rounded\", fontname=\"Helvetica\", penwidth=1.5];\n\
            \tedge [color=\"#89b4fa\", fontcolor=\"#cdd6f4\", fontname=\"Helvetica\", fontsize=9, arrowsize=0.8];\n\n"
        );

        let components: Vec<_> = crate::inventory::iter::<ProvidedComponent>().collect();
        let edges: Vec<_> = crate::inventory::iter::<DependencyEdge>().collect();

        // Identify roots/controllers for rank alignment
        let targets: std::collections::HashSet<_> = edges.iter().map(|e| e.requires).collect();
        let roots: Vec<_> = edges
            .iter()
            .map(|e| e.component)
            .filter(|c| !targets.contains(c))
            .collect();

        // Render components with high-contrast text colors
        for component in &components {
            let (bg_color, border_color, text_color, shape, label_suffix) = match component.kind {
                ComponentKind::Controller => (
                    "#2d1f3f", // Deep Purple background
                    "#cba6f7", // Mauve border
                    "#f5e0dc", // Bright off-white text
                    "box",
                    "",
                ),
                ComponentKind::Transient => (
                    "#112638", // Deep Blue background
                    "#89dceb", // Cyan border
                    "#89dceb", // Cyan text (guaranteed visible!)
                    "box",
                    " (Transient)",
                ),
                ComponentKind::Singleton => (
                    "#132a1e", // Deep Emerald background
                    "#a6e3a1", // Green border
                    "#a6e3a1", // Green text (guaranteed visible!)
                    "cylinder",
                    " (Singleton)",
                ),
                ComponentKind::Primitive => (
                    "#1e1e2e", 
                    "#6c7086", 
                    "#cdd6f4", // Crisp white/gray text
                    "ellipse",
                    "",
                ),
            };

            let _ = writeln!(
                dot,
                "\t\"{}\" [label=\"{}{}\", style=\"filled,rounded\", fillcolor=\"{}\", color=\"{}\", fontcolor=\"{}\", shape=\"{}\"];",
                component.name, component.name, label_suffix, bg_color, border_color, text_color, shape
            );
        }

        // Align top-level root endpoints on the same rank
        if !roots.is_empty() {
            dot.push_str("\n\t{ rank = same; ");
            for root in roots {
                let _ = write!(dot, "\"{}\"; ", root);
            }
            dot.push_str("}\n\n");
        }

        // Render dependency edges
        for edge in &edges {
            let _ = writeln!(
                dot,
                "\t\"{}\" -> \"{}\" [label=\"requires\"];",
                edge.component, edge.requires
            );
        }

        dot.push_str("}\n");
        dot
    }
}

/// Pair this with `inject!` once per type — this handles the compile-time bound,
/// `inject!` handles the runtime registration, and they don't have to happen in the
/// same place or at the same time.
///
/// ```ignore
/// // in redis.rs, at module scope, right after `struct RedisService`:
/// gritshield::mark_injectable!(RedisService);
/// ```
#[macro_export]
macro_rules! mark_injectable {
    ($ty:ty) => {
        impl $crate::core::ioc::RuntimeInjectable for $ty {}
    };
}

/// Registers a runtime-constructed value into the DI container, Safe to call from anywhere,
/// including inside a function.
/// This does **not** by itself satisfy a `RuntimeInjectable` bound on `$ty` — call
/// `mark_injectable!($ty)` once at module scope for that. Forgetting it means `$ty`
/// registers fine at runtime but any handler that injects it will fail to compile,
///
/// ```ignore
/// let redis_url = std::env::var("REDIS_URL").unwrap();
/// inject!(RedisService, RedisService::new(&redis_url).unwrap());
/// ```
#[macro_export]
macro_rules! inject {
    ($ty:ty, $value:expr) => {
        $crate::inventory::submit! {
            $crate::core::ioc::ProvidedComponent {
                name: std::stringify!($ty),
                kind: $crate::core::ioc::ComponentKind::Singleton,
            }
        }
        $crate::core::ioc::AutoWire::component::<$ty>($value);
    };
}