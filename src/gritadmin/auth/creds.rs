use std::sync::OnceLock;

use colored::Colorize;
use pbkdf2::pbkdf2_hmac;
use sha2::Sha256;
use subtle::ConstantTimeEq;

const HASH_SCHEME: &str = "pbkdf2-sha256";
const SALT_LEN: usize = 16;
const HASH_LEN: usize = 32;

/// OWASP-recommended iteration count for PBKDF2-HMAC-SHA256.
pub const DEFAULT_PBKDF2_ROUNDS: u32 = 210_000;

pub struct AdminCredentials {
    pub username: String,
    /// Plain-text password. Populated only outside production.
    pub password: String,
    /// `pbkdf2-sha256$<rounds>$<salt_hex>$<hash_hex>` — required in production.
    pub password_hash: Option<String>,
}

struct ParsedHash {
    rounds: u32,
    salt: Vec<u8>,
    expected: Vec<u8>,
}

/// True when `APP_ENV=production`.
pub fn is_production() -> bool {
    crate::core::env::get_env("APP_ENV", "development") == "production"
}

/// Hash a password into the value expected by `GRITSHIELD_ADMIN_PASSWORD_HASH`.
///
/// Use this in a scratch binary, or generate the same format with Python:
/// `hashlib.pbkdf2_hmac("sha256", password, salt, 210000)`.
pub fn hash_admin_password(password: &str) -> String {
    hash_admin_password_with_rounds(password, DEFAULT_PBKDF2_ROUNDS)
}

/// [`hash_admin_password`] with an explicit iteration count (useful in tests).
pub fn hash_admin_password_with_rounds(password: &str, rounds: u32) -> String {
    let salt = *uuid::Uuid::new_v4().as_bytes();
    let mut derived = [0u8; HASH_LEN];
    pbkdf2_hmac::<Sha256>(password.as_bytes(), &salt, rounds, &mut derived);
    format!(
        "{HASH_SCHEME}${rounds}${}${}",
        hex::encode(salt),
        hex::encode(derived)
    )
}

fn parse_hash(encoded: &str) -> Option<ParsedHash> {
    let mut parts = encoded.split('$');
    let (scheme, rounds, salt, hash, extra) = (
        parts.next()?,
        parts.next()?,
        parts.next()?,
        parts.next()?,
        parts.next(),
    );
    if extra.is_some() || scheme != HASH_SCHEME {
        return None;
    }
    let rounds: u32 = rounds.parse().ok()?;
    if rounds == 0 {
        return None;
    }
    let salt = hex::decode(salt).ok()?;
    let expected = hex::decode(hash).ok()?;
    if salt.len() != SALT_LEN || expected.len() != HASH_LEN {
        return None;
    }
    Some(ParsedHash {
        rounds,
        salt,
        expected,
    })
}

/// Constant-time verification of a submitted password against an encoded hash.
pub fn verify_password_hash(password: &str, encoded: &str) -> bool {
    let Some(parsed) = parse_hash(encoded) else {
        return false;
    };
    let mut derived = [0u8; HASH_LEN];
    pbkdf2_hmac::<Sha256>(password.as_bytes(), &parsed.salt, parsed.rounds, &mut derived);
    bool::from(derived.ct_eq(parsed.expected.as_slice()))
}

/// Single entry point for the login handler.
///
/// - **Development** — the plain-text password from `GRITSHIELD_ADMIN_PASSWORD`
///   (or the generated one-time password) is accepted. A configured hash is
///   also accepted, so production behaviour can be tested locally.
/// - **Production** — only `GRITSHIELD_ADMIN_PASSWORD_HASH` is consulted; the
///   plain-text env password is deliberately ignored.
pub fn verify_admin_credentials(username: &str, password: &str) -> bool {
    let admin = get_admin_credentials();
    if username != admin.username {
        return false;
    }

    if is_production() {
        return admin
            .password_hash
            .as_deref()
            .map_or(false, |hash| verify_password_hash(password, hash));
    }

    if password == admin.password {
        return true;
    }
    admin
        .password_hash
        .as_deref()
        .map_or(false, |hash| verify_password_hash(password, hash))
}

pub fn get_admin_credentials() -> &'static AdminCredentials {
    static CREDENTIALS: OnceLock<AdminCredentials> = OnceLock::new();

    CREDENTIALS.get_or_init(|| {
        let env_user = crate::core::env::get_env("GRITSHIELD_ADMIN_USER", "");
        let env_pass = crate::core::env::get_env("GRITSHIELD_ADMIN_PASSWORD", "");
        let env_hash = crate::core::env::get_env("GRITSHIELD_ADMIN_PASSWORD_HASH", "");
        let hash = if env_hash.trim().is_empty() {
            None
        } else {
            Some(env_hash.trim().to_string())
        };
        let had_user_and_pass = !env_user.is_empty() && !env_pass.is_empty();
        let username = if env_user.is_empty() {
            "admin".to_string()
        } else {
            env_user
        };

        if is_production() {
            if hash.is_none() {
                eprintln!(
                    "{}",
                    "🚨 GRITSHIELD: APP_ENV=production but GRITSHIELD_ADMIN_PASSWORD_HASH is unset. All admin logins will be denied. Set it to a pbkdf2-sha256 hash."
                        .red()
                        .bold()
                );
            } else if let Some(ref value) = hash {
                if parse_hash(value).is_none() {
                    eprintln!(
                        "{}",
                        "🚨 GRITSHIELD: GRITSHIELD_ADMIN_PASSWORD_HASH is malformed (expected pbkdf2-sha256$<rounds>$<salt>$<hash>). All admin logins will be denied."
                            .red()
                            .bold()
                    );
                }
            }

            // The plain-text password is intentionally discarded in production.
            return AdminCredentials {
                username,
                password: String::new(),
                password_hash: hash,
            };
        }

        // Development: keep the one-call plaintext convenience.
        if had_user_and_pass {
            return AdminCredentials {
                username,
                password: env_pass,
                password_hash: hash,
            };
        }

        let gen_pass = uuid::Uuid::new_v4().to_string()[..12].to_string();

        println!("\n{}", "=========================================================".yellow().bold());
        println!(" 🔐 {}", "GRITSHIELD ONE-TIME ADMIN CREDENTIALS GENERATED".yellow().bold());
        println!(" 👤 {}", format!("Username: {}", username).cyan().bold());
        println!(" 🔑 {}", format!("Password: {}", gen_pass).green().bold());
        println!(" ⚠️  {}", "To persist across restarts, set GRITSHIELD_ADMIN_USER and GRITSHIELD_ADMIN_PASSWORD in .env".dimmed());
        println!(" 🔑 {}", "In production set GRITSHIELD_ADMIN_PASSWORD_HASH instead (plain passwords are ignored).".dimmed());
        println!("{}\n", "=========================================================".yellow().bold());

        AdminCredentials {
            username,
            password: gen_pass,
            password_hash: hash,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pbkdf2_matches_known_vector() {
        let mut buf = [0u8; 20];
        pbkdf2_hmac::<Sha256>(b"password", b"salt", 600_000, &mut buf);
        assert_eq!(
            hex::encode(buf),
            "669cfe52482116fda1aa2cbe409b2f56c8e45637"
        );
    }

    #[test]
    fn hash_round_trips_and_rejects_wrong_password() {
        let encoded = hash_admin_password_with_rounds("correct horse", 1_000);
        assert!(verify_password_hash("correct horse", &encoded));
        assert!(!verify_password_hash("wrong horse", &encoded));
    }

    #[test]
    fn malformed_hashes_are_rejected() {
        assert!(!verify_password_hash("x", ""));
        assert!(!verify_password_hash("x", "sha256$1$00$00"));
        assert!(!verify_password_hash("x", "pbkdf2-sha256$0$00$00"));
        assert!(!verify_password_hash(
            "x",
            "pbkdf2-sha256$1000$nothex$00"
        ));
    }
}
