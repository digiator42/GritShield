use crate::{database::repository::ADMIN_REGISTRY, prelude::*};

/// The Master Shell Layout that loads HTMX globally.
pub fn admin_shell(title: &str, content: Markup, is_htmx: bool) -> Response {
    if is_htmx {
        return Response::ok(content.into_string());
    }

    let shell = html! {
        (maud::DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1.0";
                title { (title) " | GritAdmin" }
                script src="https://unpkg.com/htmx.org@1.9.10" {}
                script src="https://cdn.tailwindcss.com" {}
                style {
                    "
                    .htmx-indicator { display: none !important; }
                    .htmx-request .htmx-indicator, .htmx-request.htmx-indicator { display: flex !important; }
                    "
                }
            }
            body class="bg-gray-900 text-gray-100 font-sans" {
                div class="flex h-screen overflow-hidden" {
                    aside class="w-64 bg-gray-950 border-r border-gray-800 p-4" {
                        h2 class="text-xl font-bold text-emerald-400 mb-6" { "🛡️ GritAdmin" }
                        nav class="space-y-2" {

                            // DYNAMIC SIDEBAR LINKS FROM THE REGISTRY
                            @for (table_name, meta) in ADMIN_REGISTRY.lock().unwrap().iter() {
                                @let display_name = format!(
                                    "{} Records",
                                    table_name
                                        .chars()
                                        .next()
                                        .map(|c| c.to_uppercase().to_string())
                                        .unwrap_or_default()
                                        + &table_name[1..]
                                );
                                a href=(meta.route_path)
                                   hx-get=(meta.route_path)
                                   hx-target="#main-content"
                                   hx-push-url="true"
                                   class="block p-2 hover:bg-gray-800 rounded transition" { (display_name) }
                            }

                            // Static core application views can stay down here
                            hr class="border-gray-800 my-4";
                            a href="/admin/settings"
                               hx-get="/admin/settings"
                               hx-target="#main-content"
                               hx-push-url="true"
                               class="block p-2 hover:bg-gray-800 rounded transition text-gray-400" { "⚙️ System Metrics" }
                        }
                    }

                    div id="command-palette" class="hidden fixed inset-0 bg-black/60 backdrop-blur-sm z-50 flex items-start justify-center pt-20" {
                        div class="bg-gray-900 border border-gray-800 w-full max-w-2xl rounded-xl shadow-2xl overflow-hidden" {
                            div class="p-4 border-b border-gray-800 flex items-center" {
                                span class="text-xl mr-3" { "🔍" }
                                input type="text"
                                    name="q"
                                    placeholder="Search tables, settings, records..."
                                    hx-get="/admin/api/search-palette"
                                    hx-trigger="keyup changed delay:150ms"
                                    hx-target="#palette-results"
                                    class="bg-transparent text-lg text-white w-full focus:outline-none";
                                kbd class="text-xs bg-gray-800 text-gray-400 px-2 py-1 rounded shadow" { "ESC" }
                            }
                            div id="palette-results" class="max-h-96 overflow-y-auto p-2 space-y-1 text-sm text-gray-300" {
                                div class="p-4 text-center text-gray-500" { "Type to begin navigating..." }
                            }
                        }
                    }

                    main id="main-content" class="flex-1 overflow-y-auto p-8" {
                        (content)
                    }
                }
                div id="toast-container" class="fixed bottom-4 right-4 z-50 space-y-2" { }
            }
            script {
                (maud::PreEscaped(include_str!("admin_palette.js")))
            }
        }
    };

    Response::ok(shell.into_string())
}
