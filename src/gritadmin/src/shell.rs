use gritshield::prelude::*;

/// The Master Shell Layout that loads HTMX globally.
/// If an incoming request is an HTMX request (`HX-Request` header present),
/// we omit the shell and just return the raw inner content block for instant swap!
pub fn admin_shell(title: &str, content: Markup, is_htmx: bool) -> Response {
    if is_htmx {
        // Return only the partial component chunk for HTMX to swap into the DOM
        return Response::ok(content.into_string());
    }

    // Full page layout rendering on direct URL hits or manual browser refreshes
    let shell = html! {
        (maud::DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1.0";
                title { (title) " | GritAdmin" }
                // Load HTMX & Tailwind styles via CDN safely
                script src="https://unpkg.com/htmx.org@1.9.10" {}
                script src="https://cdn.tailwindcss.com" {}
                style {
    "
    .htmx-indicator {
        display: none !important;
    }
    /* When an active request is running, reveal elements inside or marked by it */
    .htmx-request .htmx-indicator, 
    .htmx-request.htmx-indicator {
        display: flex !important;
    }
    "
}
            }
            body class="bg-gray-900 text-gray-100 font-sans" {
                div class="flex h-screen overflow-hidden" {
                    // Left navigation bar links use hx-get and target the #main-content container
                    aside class="w-64 bg-gray-950 border-r border-gray-800 p-4" {
                        h2 class="text-xl font-bold text-emerald-400 mb-6" { "🛡️ GritAdmin" }
                        nav class="space-y-2" {
                            a href="/admin/users"
                               hx-get="/admin/users"
                               hx-target="#main-content"
                               hx-push-url="true"
                               class="block p-2 hover:bg-gray-800 rounded transition" { "User Records" }

                            a href="/admin/settings"
                               hx-get="/admin/settings"
                               hx-target="#main-content"
                               hx-push-url="true"
                               class="block p-2 hover:bg-gray-800 rounded transition" { "System Metrics" }
                        }
                    }

                    // Hidden overlay container that reveals itself via JavaScript toggle or simple CSS classes
                    div id="command-palette" class="hidden fixed inset-0 bg-black/60 backdrop-blur-sm z-50 flex items-start justify-center pt-20" {
                        div class="bg-gray-900 border border-gray-800 w-full max-w-2xl rounded-xl shadow-2xl overflow-hidden animate-in fade-in zoom-in-95 duration-150" {
                            div class="p-4 border-b border-gray-800 flex items-center" {
                                span class="text-xl mr-3 text-gray-500" { "🔍" }
                                input type="text"
                                    name="q"
                                    placeholder="Search tables, settings, records..."
                                    hx-get="/admin/api/search-palette"
                                    hx-trigger="keyup changed delay:150ms"
                                    hx-target="#palette-results"
                                    class="bg-transparent text-lg text-white w-full focus:outline-none";
                                kbd class="text-xs bg-gray-800 text-gray-400 px-2 py-1 rounded shadow" { "ESC" }
                            }
                            // Dynamic results container populated instantly by HTMX
                            div id="palette-results" class="max-h-96 overflow-y-auto p-2 space-y-1 text-sm text-gray-300" {
                                div class="p-4 text-center text-gray-500" { "Type to begin navigating..." }
                            }
                        }
                    }

                    // The SPA Main Target Content Window Workspace
                    main id="main-content" class="flex-1 overflow-y-auto p-8" {
                        (content)
                    }
                }
            }
            // Small clean Global Event Listener script to intercept Cmd + K
            script {
                (maud::PreEscaped(include_str!("static/admin_palette.js")))
            }
        }
    };

    Response::ok(shell.into_string())
}
