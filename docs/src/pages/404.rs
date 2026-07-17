use crate::root::layout::main_layout;
use gritshield::http::response::Response;
use gritshield::routing::engine::RequestContext;
use gritshield::security::xss::Sanitizer;
use maud::html;

pub async fn handler(ctx: RequestContext) -> Response {
    let body_markup = html! {
        div class="my-auto p-12 text-center max-w-md mx-auto my-8 border border-slate-800 bg-slate-900/40 rounded-2xl" {
            span class="text-4xl" { "🛰️" }
            h1 class="text-xl font-bold text-slate-100 mt-4" { "Lost in Orbit" }
            p class="text-xs text-slate-400 mt-2 leading-relaxed" {
                "404 NOT FOUND."
            }
            a href="/docs/index" class="mt-6 inline-block text-xs font-semibold text-indigo-400 hover:underline" {
                "← Return to docs"
            }
        }
    };

    let full_html_page = main_layout("404 Not Found", body_markup, &ctx);

    let res = Response::new(404, Sanitizer::trust(&full_html_page.into_string()));
    res
}

gritshield::register_fallback_page!(handler);
