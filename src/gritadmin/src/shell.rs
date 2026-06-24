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

                    // The SPA Main Target Content Window Workspace
                    main id="main-content" class="flex-1 overflow-y-auto p-8" {
                        (content)
                    }
                }
            }
        }
    };

    Response::ok(shell.into_string())
}
