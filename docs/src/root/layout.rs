use gritshield::prelude::*;

pub fn main_layout(title: &str, content: maud::Markup, _: &RequestContext) -> maud::Markup {
    html! {
        (maud::DOCTYPE)
        html {
            head {
                title { (title) }
                link rel="stylesheet" href="/static/style.css";
                script src="https://cdn.tailwindcss.com" {}
            }
            body {
                main class="min-h-screen bg-slate-900 text-slate-50 flex justify-center items-center p-8" {
                    div class="max-w-7xl w-full bg-slate-800 p-8 rounded-xl shadow-lg" {
                        (content)
                    }
                }
                footer {
                    p { "Crafted safely with Gritshield Web Engine" }
                }
            }
        }
    }
}
