#[macro_export]
macro_rules! render {
    ($title:expr, $markup:expr) => {{
        use crate::templates::layout::main_layout;
        let final_html = main_layout($title, $markup).into_string();
        Response::new(200, Sanitizer::trust(&final_html))
    }};
}
