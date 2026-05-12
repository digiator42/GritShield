use crate::security::xss::{SafeHtml, Sanitizer, UntrustedString};
use std::collections::HashMap;

pub fn profile_handler(params: HashMap<String, UntrustedString>) -> SafeHtml {
    let name = params.get("name").cloned().unwrap();
    let safe_name = Sanitizer::encode(name);

    Sanitizer::trust(&format!(
        "<h1>Profile Page</h1><p>Welcome, {}!</p>",
        safe_name
    ))
}
