# GritShield Dependency Injection (DI) Engine

GritShield offers a unique **Dual-Engine Dependency Injection** architecture. Depending on your system design and performance requirement, you can choose between two completely standalone paradigms:


## Paradigm A: The Dynamic / Inventory Way

In this paradigm, GritShield handles the plumbing behind the scenes using dynamic registration. You decorate your structs, and the runtime container scans and hooks them up automatically.

- With the inventory magic, main.rs stays completely clean and never changes. You can add 50 new controllers across 50 different source files. As long as they use your `#[derive(GritComponent)]` or `#[controller]` tags, GritShield discovers them, sets up their routing links, and injects them completely in the background.
- Refere to usage [Macro Library](/docs/04_macros_library/dependency_injection_macros.html#paradigm-a)

## Paradigm B: The Strict Compile-Time Pathway

If you want absolute architectural control with **zero runtime overhead** and **100% type safety**, you can entirely bypass the background reflection magic. By declaring an application container and explicit route handlers, any missing dependency becomes a **hard compiler error**.


- With compile time wire, it solves the "Boilerplate Explosion", Imagine an application with 30 components (Database, Redis, EmailService, Logger, AuthService, OrderRepository, ProductRepository, OrderController, UserController, etc.).

- Without `WireContainer`, your `main.rs` manual construction file becomes a massive, ordered mess where you have to manually pass every single nested field down a deeply nested tree:

    ```rust
    // Without WireContainer, you have to manage the ordering and nesting manually:
    let logger = Arc::new(Logger::new());
    let db = Arc::new(DatabasePool::new(logger.clone()));
    let redis = Arc::new(RedisService::new());
    let email = Arc::new(EmailService::new(logger.clone()));
    let order_repo = Arc::new(OrderRepository::new(db.clone(), logger.clone()));
    let payment = Arc::new(PaymentService::new(redis.clone()));
    ```

    ```rust
    // Boilerplate: You have to explicitly type out every field for every controller
    let order_controller = Arc::new(OrderController {
        db: db.clone(),
        ps: payment.clone(),
        logger: logger.clone(),
        repo: order_repo.clone(),
    });
    ```

    With `WireContainer`, the construction is completely flat. You instantiate your dependencies once into a single struct, and the macro handles the structural binding boilerplate automatically:

    Rust

    ```rust
    let container = AppContainer { db, redis, logger, email, order_repo, payment };

    // Automatically knows what fields OrderController needs and pulls them from the container
    let order_controller = OrderController::compile_time_wire(&container);
    let user_controller = UserController::compile_time_wire(&container);
    ```