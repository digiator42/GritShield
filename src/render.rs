
#[macro_export]
macro_rules! render {
    // Mode A: Standard Maud Markup Render
    ($ctx:expr, $title:expr, $markup:expr) => {{
        // Pass a reference to the context into main_layout
        let final_html = crate::root::layout::main_layout($title, $markup, &$ctx).into_string();

        $crate::protocol::response::Response::new(
            200,
            $crate::security::xss::Sanitizer::trust(&final_html),
        )
    }};

    // Mode B: Raw HTML String Injection via explicit token flag
    (raw, $ctx:expr, $title:expr, $html_string:expr) => {{
        let raw_wrapper = maud::PreEscaped($html_string);
        let final_html = crate::root::layout::main_layout($title, raw_wrapper, &$ctx).into_string();

        $crate::protocol::response::Response::new(
            200,
            $crate::security::xss::Sanitizer::trust(&final_html),
        )
    }};
}
