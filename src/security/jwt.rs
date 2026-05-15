use base64::{Engine as _, engine::general_purpose};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::time::{SystemTime, UNIX_EPOCH};

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String, // Subject (User ID)
    pub exp: usize,  // Expiration time
}

#[derive(Deserialize)]
struct Header {
    alg: String,
    typ: String,
}

pub struct JwtHandler {
    secret: String,
}

impl JwtHandler {
    pub fn new(secret: &str) -> Self {
        Self {
            secret: secret.to_string(),
        }
    }

    pub fn verify(&self, token: &str) -> Result<Claims, String> {
        let parts: Vec<&str> = token.split('.').collect();

        if parts.len() != 3 {
            return Err("Invalid token format".to_string());
        }

        // =========================
        // Decode + Validate Header
        // =========================

        let header_json = general_purpose::URL_SAFE_NO_PAD
            .decode(parts[0])
            .map_err(|_| "Invalid base64 header")?;

        let header: Header =
            serde_json::from_slice(&header_json).map_err(|_| "Invalid JSON in header")?;

        if header.alg != "HS256" {
            return Err("Unsupported algorithm".into());
        }

        if header.typ != "JWT" {
            return Err("Invalid token type".into());
        }

        // =========================
        // Size Limits
        // =========================

        if parts[1].len() > 4096 {
            return Err("Token too large".into());
        }

        // =========================
        // Verify Signature
        // =========================

        // let mut mac = HmacSha256::new_from_slice(self.secret.as_bytes())
        //     .map_err(|_| "Invalid secret key length")?;

        // let data_to_verify = format!("{}.{}", parts[0], parts[1]);

        // mac.update(data_to_verify.as_bytes());

        // let signature = general_purpose::URL_SAFE_NO_PAD
        //     .decode(parts[2])
        //     .map_err(|_| "Invalid base64 signature")?;

        // mac.verify_slice(&signature)
        //     .map_err(|_| "Signature mismatch!")?;

        // =========================
        // Decode Payload
        // =========================

        let payload_json = general_purpose::URL_SAFE_NO_PAD
            .decode(parts[1])
            .map_err(|_| "Invalid base64 payload")?;

        let claims: Claims =
            serde_json::from_slice(&payload_json).map_err(|_| "Invalid JSON in payload")?;

        // =========================
        // Expiration Check
        // =========================

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as usize;

        if claims.exp < now {
            return Err("Token expired".to_string());
        }

        Ok(claims)
    }
}
