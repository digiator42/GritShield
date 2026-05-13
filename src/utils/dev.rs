use crate::{
    protocol::response::{Cookie, Response},
    routing::trie::RequestContext,
    security::xss::{SafeHtml, Sanitizer},
};

pub fn profile_handler(ctx: RequestContext) -> Response {
    let name = ctx.params.get("name").unwrap();

    if name.as_str() == "logo" {
        return Response::static_file("static/img/logo.png");
    }

    // Returns the Html variant of Response
    let mut res = Response::new(200, Sanitizer::trust("<h1>User Profile</h1>"));
    res.cookies.push(Cookie::new("JSESSIONID", "2024-10-01"));
    res
}

pub fn products_handler(_: RequestContext) -> Response {
    let body = Sanitizer::trust(&format!("<h1>products Page</h1><p>Welcome!</p>"));
    Response::new(200, body)
}

pub fn static_handler(ctx: RequestContext) -> Response {
    // let path = ctx.params.get("path").unwrap();
    let body = Sanitizer::trust("/* Static Content Rendered */");
    Response::new(200, body)
}

pub fn dashboard_handler(ctx: RequestContext) -> Response {
    if let Some(session_ptr) = ctx.session {
        let mut session = session_ptr.lock().unwrap();
        session
            .data
            .insert("last_action".to_string(), "view_dashboard".to_string());

        return Response::new(200, Sanitizer::trust("<h1>Welcome Back!</h1>"));
    }

    Response::new(401, Sanitizer::trust("<h1>Session Required</h1>"))
}
