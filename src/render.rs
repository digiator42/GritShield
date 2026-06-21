#[macro_export]
macro_rules! render {
    // Mode A: Standard Maud Markup Render (Async)
    ($ctx:expr, $title:expr, $markup:expr) => {{
        // Await the main_layout async function
        let final_html = crate::root::layout::main_layout($title, $markup, &$ctx)
            .await
            .into_string();

        $crate::protocol::response::Response::new(
            200,
            $crate::security::xss::Sanitizer::trust(&final_html),
        )
    }};

    // Mode B: Raw HTML String Injection via explicit token flag
    ($ctx:expr, $title:expr, raw $html_string:expr) => {{
        let raw_wrapper = maud::PreEscaped($html_string);
        let final_html = crate::root::layout::main_layout($title, raw_wrapper, &$ctx)
            .await
            .into_string();

        $crate::protocol::response::Response::new(
            200,
            $crate::security::xss::Sanitizer::trust(&final_html),
        )
    }};
}
