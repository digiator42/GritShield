# Configuration

GritShield uses environment variables for configuration with `.env` file support.

## Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `APP_ENV` | `development` | `development` or `production` |
| `JWT_SECRET` | `fallback_secure_key_string` | Secret for JWT and signed cookies |
| `DATABASE_URL` | None | Database connection string |

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