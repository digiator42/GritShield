use crate::{
    routing::trie::RequestContext,
    security::xss::{SafeHtml, Sanitizer},
};

pub fn profile_handler(ctx: RequestContext) -> SafeHtml {
    let name = ctx.params.get("name").cloned().unwrap();
    let safe_name = Sanitizer::encode(name);

    Sanitizer::trust(&format!(
        "<h1>Profile Page</h1><p>Welcome, {}!</p>",
        safe_name
    ))
}

pub fn products_handler(_: RequestContext) -> SafeHtml {
    Sanitizer::trust(&format!("<h1>products Page</h1><p>Welcome!</p>"))
}

pub fn static_handler(ctx: RequestContext) -> SafeHtml {
    // let path = ctx.params.get("path").unwrap();
    Sanitizer::trust("/* Static Content Rendered */")
}
