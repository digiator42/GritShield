use base64::{Engine as _, engine::general_purpose};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    pub sub: String, // Subject (User ID)
    pub role: String, // Stateless Role Claim tracking
    pub exp: usize,  // Expiration time
}

impl Claims {
    pub fn new(sub: String, role: String, duration: u64) -> Self {
        let now = SystemTime::now();
        let exp = now
            .checked_add(Duration::from_secs(duration))
            .unwrap()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        Claims {
            sub: sub,
            role: role,
            exp: exp as usize,
        }
    }
}

#[derive(Deserialize, Serialize)]
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

    /// Encodes and signs a Claims payload into a valid JWT token string.
    pub fn sign(&self, claims: &Claims) -> Result<String, String> {
        // Build and serialize the standard header
        let header = Header {
            alg: "HS256".to_string(),
            typ: "JWT".to_string(),
        };

        let header_json =
            serde_json::to_string(&header).map_err(|_| "Failed to serialize header")?;
        let payload_json =
            serde_json::to_string(claims).map_err(|_| "Failed to serialize payload")?;

        // Base64 URL-safe Encode Header and Payload
        let encoded_header = general_purpose::URL_SAFE_NO_PAD.encode(header_json.as_bytes());
        let encoded_payload = general_purpose::URL_SAFE_NO_PAD.encode(payload_json.as_bytes());

        // Prepare signature data structure block
        let data_to_sign = format!("{}.{}", encoded_header, encoded_payload);

        // Compute HMAC-SHA256 signature
        // Use the same instantiation pattern fixed above
        let mut mac = HmacSha256::new_from_slice(self.secret.as_bytes()) // or new_varkey
            .map_err(|_| "Invalid secret key length")?;

        mac.update(data_to_sign.as_bytes());
        let mac_result = mac.finalize();
        let signature_bytes = mac_result.into_bytes();

        let encoded_signature = general_purpose::URL_SAFE_NO_PAD.encode(&signature_bytes);

        // 5. Return the fully concatenated token string
        Ok(format!("{}.{}", data_to_sign, encoded_signature))
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

        let mut mac = HmacSha256::new_from_slice(self.secret.as_bytes())
            .map_err(|_| "Invalid secret key length")?;

        let data_to_verify = format!("{}.{}", parts[0], parts[1]);

        mac.update(data_to_verify.as_bytes());

        let signature = general_purpose::URL_SAFE_NO_PAD
            .decode(parts[2])
            .map_err(|_| "Invalid base64 signature")?;

        mac.verify_slice(&signature)
            .map_err(|_| "Signature mismatch!")?;

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
