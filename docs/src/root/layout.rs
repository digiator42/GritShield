use gritshield::prelude::*;

pub fn main_layout(title: &str, content: maud::Markup) -> maud::Markup {
    html! {
        (maud::DOCTYPE)
        html {
            head {
                title { (title) }
                link rel="stylesheet" href="/static/style.css";
            }
            body {
                main class="container" {
                    (content)
                }
                footer {
                    p { "Crafted safely with Gritshield Web Engine" }
                }
            }
        }
    }
}
