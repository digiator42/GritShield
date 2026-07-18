# Configuration

GritShield uses simple environment variables for configuration with `.env` file support, for most of cases configuration is done programmatically.

## Hot Reload (Development)

```rust
//          localhost,  port,  router
ignite("127.0.0.1", "8080", router).await;
```

Automatically rebuilds and reloads on source changes.

## Environment Variables

| Variable       | Default                      | Description                       |
| -------------- | ---------------------------- | --------------------------------- |
| `APP_ENV`      | `development`                | `development` or `production`     |
| `JWT_SECRET`   | `fallback_secure_key_string` | Secret for JWT and signed cookies |
| `DATABASE_URL` | None                         | Database connection string        |

## .env File

Create `.env` in your project root:

```env
APP_ENV=development
JWT_SECRET=your-super-secret-key-minimum-32-chars
DATABASE_URL=postgres://user:pass@localhost/mydb
```

## Reading Configuration

```rust
use gritshield::core::env::get_env;

let db_url = get_env("DATABASE_URL", "sqlite::memory:");
let is_production = get_env("APP_ENV", "development") == "production";
```

## Production Settings

For production, always set:

```env
APP_ENV=production
JWT_SECRET=<random-32+-byte-string>
```

Production mode automatically:

- Disables debug output in error pages
- Enforces secure cookies
- Uses strict SameSite policies
- Caches templates in memory

## Custom Configuration

Extend with your own env vars:

```env
REDIS_URL=redis://localhost:6379
S3_BUCKET=myapp-uploads
MAILER_DSN=smtp://user:pass@mailhog:1025
```

Access them with `get_env()` or `std::env::var()` directly.

## Configure Logger

```rust
// Adds logger info for each request, status, cookies, ...
let mut router = Router::new().mount_logger(LogLevel::Debug);

// Each level gives you the lower levels logs, Debug shows Info, Warn, Error logs
pub enum LogLevel {
    Off = 0,
    Error = 1,
    Warn = 2,
    Info = 3,
    Debug = 4,
    Trace = 5,
}

//     localhost,  port,  router
ignite("127.0.0.1", "8080", router).await;
```

Or simply add .env var

```env
GRIT_LOG=debug
```
