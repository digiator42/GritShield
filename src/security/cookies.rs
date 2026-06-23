use crate::{
    core::env::get_env,
    protocol::response::{Cookie, Response, SameSite}, warn,
};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::collections::HashMap;

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Clone)]
pub struct CookieJar {
    incoming: HashMap<String, String>,
    outgoing: HashMap<String, Cookie>,
    signing_key: String,
}

impl CookieJar {
    /// Instantiates a jar by extracting the raw browser header values
    pub fn new(cookie_header: Option<&String>, signing_key: String) -> Self {
        let mut incoming = HashMap::new();

        if let Some(header_val) = cookie_header {
            // Parse typical format: "session_id=123; theme=dark"
            for pair in header_val.split(';') {
                let mut parts = pair.splitn(2, '=');
                if let (Some(k), Some(v)) = (parts.next(), parts.next()) {
                    incoming.insert(k.trim().to_string(), v.trim().to_string());
                }
            }
        }

        Self {
            incoming,
            outgoing: HashMap::new(),
            signing_key,
        }
    }

    /// Read a standard, unencrypted cookie value
    pub fn get(&self, name: &str) -> Option<&String> {
        self.incoming.get(name)
    }

    /// Stage a new cookie to be sent back to the browser client
    pub fn add(&mut self, cookie: Cookie) {
        self.outgoing.insert(cookie.name.clone(), cookie);
    }

    /// Read a signed cookie, ensuring it hasn't been modified by the browser
    pub fn get_signed(&self, name: &str) -> Option<String> {
        let raw_val = self.incoming.get(name)?;

        // Signed cookies follow a "value.signature" protocol format
        let mut parts = raw_val.splitn(2, '.');
        let value = parts.next()?;
        let signature = parts.next()?;

        // Recompute expected HMAC signature
        if self.verify_signature(value, signature) {
            Some(value.to_string())
        } else {
            warn!(
                "[SECURITY WARNING] Cookie tampering detected for: {}",
                name
            );
            None
        }
    }

    /// Stage an encrypted/signed cookie with an active HMAC block
    pub fn add_signed(&mut self, mut cookie: Cookie) {
        let signed_value = self.compute_signature(&cookie.value);
        cookie.value = format!("{}.{}", cookie.value, signed_value);
        self.add(cookie);
    }

    /// Instructs the browser to wipe the target key immediately
    pub fn remove(&mut self, name: &str) {
        // Detect if we are running in local development
        let is_production = get_env("APP_ENV", "development") == "production";

        // Build the tombstone deletion cookie
        let mut tombstone = Cookie::new(name, "");
        tombstone.max_age = 0; // Tells browser to wipe it instantly

        // FIX: Adjust safety restrictions so localhost HTTP can actually process the deletion!
        tombstone.secure = is_production;
        tombstone.same_site = if is_production {
            SameSite::Strict
        } else {
            SameSite::Lax
        };

        // Stage it to be sent back over the wire
        self.outgoing.insert(name.to_string(), tombstone);
    }

    /// Consumes the jar and drains all outgoing cookies into the target response pipeline
    pub fn commit(self, mut response: Response) -> Response {
        for (_, cookie) in self.outgoing {
            response = response.with_cookie(cookie);
        }
        response
    }

    fn compute_signature(&self, value: &str) -> String {
        let mut hmac = HmacSha256::new_from_slice(self.signing_key.as_bytes())
            .expect("HMAC can accept keys of any size");

        hmac.update(value.as_bytes());

        // Finalize the calculation and extract the code bytes
        let result = hmac.finalize();
        let code_bytes = result.into_bytes();

        // Encode the byte slice into a hex string
        hex::encode(code_bytes)
    }

    pub fn verify_signature(&self, value: &str, expected_hex_sig: &str) -> bool {
        let mut mac = HmacSha256::new_from_slice(self.signing_key.as_bytes()).unwrap();
        mac.update(value.as_bytes());

        // Decode the signature you are checking against back into bytes
        if let Ok(expected_bytes) = hex::decode(expected_hex_sig) {
            // .verify_slice() evaluates in strictly constant-time!
            mac.verify_slice(&expected_bytes).is_ok()
        } else {
            false
        }
    }
}
