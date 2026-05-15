#[macro_export]
macro_rules! render {
    ($title:expr, $markup:expr) => {{
        // in the developer's app, not the framework.
        let final_html = crate::templates::layout::main_layout($title, $markup).into_string();

        $crate::protocol::response::Response::new(
            200,
            $crate::security::xss::Sanitizer::trust(&final_html),
        )
    }};
}
