use maud::{html, Markup, DOCTYPE};

pub fn main_layout(title: &str, content: Markup) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                title { (title) }
                link rel="stylesheet" href="/static/css/style.css";
            }
            body {
                nav {
                    a href="/" { "Home" }
                    a href="/docs/intro" { "Documentation" }
                }
                main {
                    (content)
                }
                footer {
                    p { "Built with our Custom Rust Kernel" }
                }
            }
        }
    }
}