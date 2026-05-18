#[macro_export]
macro_rules! render {
    // Mode A: Standard Maud Markup Render (Default behavior)
    ($title:expr, $markup:expr) => {{
        let final_html = crate::root::layout::main_layout($title, $markup).into_string();

        $crate::protocol::response::Response::new(
            200,
            $crate::security::xss::Sanitizer::trust(&final_html),
        )
    }};

    // Mode B: Raw HTML String Injection via explicit token flag
    (raw, $title:expr, $html_string:expr) => {{
        // Wrap the raw string in PreEscaped so Maud skips safety escaping
        let raw_wrapper = maud::PreEscaped($html_string);
        let final_html = crate::root::layout::main_layout($title, raw_wrapper).into_string();

        $crate::protocol::response::Response::new(
            200,
            $crate::security::xss::Sanitizer::trust(&final_html),
        )
    }};
}
