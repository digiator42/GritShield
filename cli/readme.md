# GritShield CLI

Command-line developer utility tool for building robust, secure web services with the GritShield framework kernel.

## Installation

Install the binary executable globally using Cargo:
```bash
cargo install gritshield_cli
```

## Commands

### `new` — Scaffold a new project

Create a fresh GritShield application interactively (database engine selection prompt):

```bash
# Basic project (SQLite, no admin)
gritshield new secure_app

# With admin panel and swagger features
gritshield new secure_app --admin --swagger
```

Options:
- `-a, --admin` — Enable the admin panel feature
- `-s, --swagger` — Enable the swagger/OpenAPI feature

### `generate` (alias: `gen`) — Generate framework structures

Scaffold individual components, models, controllers, and more:

```bash
# Controllers
gritshield gen controller user          # Creates src/controllers/user.rs
gritshield gen controller admin_panel   # Creates src/controllers/admin_panel.rs

# Models (SeaORM entity with GritModel derive)
gritshield gen model user               # Creates src/models/user.rs

# Repositories (GritAdmin + GritComponent annotated)
gritshield gen repository user          # Creates src/repositories/user.rs

# Components (GritComponent for DI wiring)
gritshield gen component email_service  # Creates src/services/email_service.rs

# Events (GritEvent + handler)
gritshield gen event user_registered    # Creates src/events/user_registered.rs

# Jobs (GritJob with retry config)
gritshield gen job send_email           # Creates src/jobs/send_email.rs

# Interceptors (AOP Interceptor trait impl)
gritshield gen interceptor audit_logger # Creates src/interceptors/audit_logger.rs

# Catch handlers
gritshield gen catch 404                # Creates src/catch.rs

# Security capabilities
gritshield gen caps manage_billing      # Creates src/security/caps.rs

# Migrations
gritshield gen migration add_users_table
```

### `migration` — Run database migrations

Apply or rollback migrations against your database:

```bash
# Apply all pending migrations
gritshield migration up

# Rollback the last migration
gritshield migration down

# Apply a specific migration file
gritshield migration up --file 20240101_120000_add_users_table.sql

# Rollback a specific migration file
gritshield migration down --file 20240101_120000_add_users_table.sql
```

### `diag` — Inspect DI container and routing topology

Export the auto-discovered dependency injection graph:

```bash
# Export as Graphviz DOT
gritshield diag --dot

# Export as Mermaid markdown
gritshield diag --mermaid
```

## Full Workflow Example

```bash
# 1. Create a new project with admin + swagger
gritshield new my_app --admin --swagger
cd my_app

# 2. Generate models and repositories
gritshield gen model user
gritshield gen repository user
gritshield gen model post
gritshield gen repository post

# 3. Generate controllers
gritshield gen controller user
gritshield gen controller post

# 4. Generate a background job and event
gritshield gen job send_welcome_email
gritshield gen event user_registered

# 5. Add security capabilities
gritshield gen caps manage_billing

# 6. Run migrations
gritshield migration up

# 7. Start the server
cargo run
```

## Generated Project Structure

```
my_app/
├── Cargo.toml          # gritshield = "0.2.2" dependency
├── .env                # Environment configuration
├── src/
│   ├── main.rs         # Entry point using #[launch] + Shield::build()
│   ├── controllers/
│   │   ├── mod.rs
│   │   ├── info.rs     # Default API controller
│   │   └── user.rs     # Your generated controllers
│   ├── models/         # SeaORM entities + GritModel
│   ├── repositories/   # GritAdmin + GritComponent repos
│   ├── services/       # GritComponent DI services
│   ├── events/         # GritEvent structs + handlers
│   ├── jobs/           # GritJob structs + handlers
│   ├── interceptors/   # AOP Interceptor impls
│   ├── security/
│   │   ├── mod.rs
│   │   └── caps.rs     # declare_security_caps! module
│   └── catch.rs        # Custom HTTP error handlers
├── migrations/         # SQL migration files (-- Up: / -- Down:)
└── static/             # Static file assets
    └── css/
```
