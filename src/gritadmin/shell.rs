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
                    /* 1. Hide all indicators by default everywhere */
                    .htmx-indicator, 
                    #search-spinner, 
                    .htmx-indicator-bar { 
                        display: none !important; 
                    }

                    /* 2. Global Top Progress Bar - only show when body or a link is working */
                    body.htmx-request .htmx-indicator-bar,
                    .htmx-request.htmx-indicator-bar {
                        display: block !important;
                    }

                    /* 3. Local Search Input Spinner - only show when search is working */
                    #search-spinner.htmx-request {
                        display: block !important;
                    }

                    /* 4. JQL Explorer Spinner - ONLY show when the container itself is loading */
                    #jql-container.htmx-request .htmx-indicator {
                        display: flex !important;
                    }

                    /* A global top progress indicator bar */
                    .htmx-indicator-bar {
                        display: none;
                        position: fixed;
                        top: 0;
                        left: 0;
                        width: 100%;
                        height: 4px;
                        background: linear-gradient(100deg, #f6dc9f, #fb923c, #b9f318);
                        background-size: 100% 200%;
                        animation: loading-bar-move 1.5s infinite linear;
                        z-index: 9999;
                    }
                    .htmx-request.htmx-indicator-bar, 
                    .htmx-request .htmx-indicator-bar {
                        display: block;
                    }

                    @keyframes loading-bar-move {
                        0% { background-position: 0% 0%; }
                        100% { background-position: 0% -200%; }
                    }

                    /* Local CSS Spinner rotation */
                    @keyframes spin {
                        to { transform: rotate(360deg); }
                    }
                    .animate-spin-custom {
                        animation: spin 1s linear infinite;
                    }

                    /* Dim elements while waiting */
                    .htmx-request.opacity-changing {
                        opacity: 0.6;
                        pointer-events: none;
                    }
                    /* ─── Custom Scrollbar ─── */
                    /* WebKit/Blink (Chrome, Edge, Safari) */
                    ::-webkit-scrollbar {
                        width: 10px;
                        height: 10px;
                    }

                    ::-webkit-scrollbar-track {
                        background: #1a1a1a;
                        border-radius: 8px;
                    }

                    ::-webkit-scrollbar-thumb {
                        background: linear-gradient(180deg, #f97316, #dc2626);
                        border-radius: 8px;
                        border: 2px solid #1a1a1a;
                    }

                    ::-webkit-scrollbar-thumb:hover {
                        background: linear-gradient(180deg, #fb923c, #ef4444);
                    }

                    ::-webkit-scrollbar-corner {
                        background: transparent;
                    }

                    /* ─── Firefox ─── */
                    * {
                        scrollbar-width: thin;
                        scrollbar-color: #f97316 #1a1a1a;
                    }

                    /* ─── Table Scrollbar (Override) ─── */
                    .table-scroll::-webkit-scrollbar {
                        width: 8px;
                        height: 8px;
                    }

                    .table-scroll::-webkit-scrollbar-track {
                        background: #0d0d0d;
                        border-radius: 6px;
                    }

                    .table-scroll::-webkit-scrollbar-thumb {
                        background: linear-gradient(180deg, #f59e0b, #dc2626);
                        border-radius: 6px;
                        border: 1px solid #0d0d0d;
                    }

                    .table-scroll::-webkit-scrollbar-thumb:hover {
                        background: linear-gradient(180deg, #fbbf24, #ef4444);
                    }

                    .table-scroll {
                        scrollbar-width: thin;
                        scrollbar-color: #f59e0b #0d0d0d;
                    }
                    "
                }
            }
            body class="bg-gray-900 text-gray-100 font-sans" {
                div class="flex h-screen overflow-hidden" {
                    aside class="w-72 bg-gray-950 border-r border-gray-800 p-4" {
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
                                ).replace("_", "");
                                a href=(meta.route_path)
                                   hx-get=(meta.route_path)
                                   hx-target="#main-content"
                                   hx-indicator="body"
                                   hx-push-url="true"
                                   class="block p-2 hover:bg-gray-800 rounded transition" { (display_name) }
                            }

                            // Static core application views can stay down here
                            hr class="border-gray-800 my-4";
                            a href="/admin/dashboard"
                            hx-get="/admin/dashboard"
                            hx-target="#main-content"
                            hx-indicator="body"
                            hx-push-url="true"
                            class="block p-2 hover:bg-gray-800 rounded transition text-gray-400" { "📊 Dashboard" }

                            a href="/admin/api/metrics/html"
                               hx-get="/admin/api/metrics/html"
                               hx-target="#main-content"
                               hx-push-url="true"
                               class="block p-2 hover:bg-gray-800 rounded transition text-gray-400" { "⚙️ System Metrics" }
                        }
                        hr class="border-gray-800 my-4";
                        div class="flex items-center justify-between" {
                            // Button to open the luxury dynamic builder modal
                            button 
                                onclick="document.getElementById('schema-modal').classList.remove('hidden')"
                                class="bg-emerald-950/40 w-full border border-emerald-800/60 hover:bg-emerald-900/40 text-emerald-400 text-xxs font-mono font-semibold px-3 py-1.5 rounded-lg transition duration-150 shadow-md" {
                                "Create Table"
                            }
                        }

                        // --- LUXURY SCHEMA BUILDER MODAL OVERLAY ---
                        div id="schema-modal" class="hidden fixed inset-0 z-50 flex items-center justify-center bg-black/80 backdrop-blur-sm p-4 animate-fade-in" {
                            div class="bg-gray-950 border border-gray-800 rounded-2xl max-w-2xl w-full max-h-[85vh] flex flex-col shadow-2xl overflow-hidden" {
                                
                                // Modal Header
                                div class="p-5 border-b border-gray-800 flex justify-between items-center bg-gray-900/40" {
                                    div {
                                        h3 class="text-sm font-bold font-mono text-emerald-400" { "Matrix Entity Creator" }
                                        p class="text-xxs font-mono text-gray-400 mt-0.5" { "Configure column types safely across PostgreSQL, MySQL & SQLite" }
                                    }
                                    button 
                                        onclick="document.getElementById('schema-modal').classList.add('hidden')"
                                        class="text-gray-500 hover:text-white transition text-sm font-mono p-1" { "✕" }
                                }

                                // Modal Body (Form Container)
                                form hx-post="/admin/api/create-table"
                                    hx-target="#main-content"
                                    hx-indicator="body"
                                    onsubmit="document.getElementById('schema-modal').classList.add('hidden');"
                                    class="flex-1 overflow-y-auto p-6 space-y-5" {
                                    
                                    // Table Name Configuration
                                    div class="space-y-1.5" {
                                        label class="block text-xxs font-mono font-semibold uppercase tracking-wider text-gray-400" { "Table Identity" }
                                        input type="text"
                                            name="table_name"
                                            required
                                            placeholder="e.g., user_profiles"
                                            class="bg-gray-900 border border-gray-800 rounded-lg px-4 py-2.5 w-full text-xs font-mono text-emerald-400 focus:outline-none focus:border-emerald-500 placeholder-gray-700 shadow-inner";
                                    }

                                    // Columns Specification Track
                                    div class="space-y-3" {
                                        div class="flex items-center justify-between" {
                                            label class="text-xxs font-mono font-semibold uppercase tracking-wider text-gray-400" { "Attributes Matrix" }
                                            button 
                                                type="button"
                                                onclick="addSchemaColumnRow()"
                                                class="text-blue-400 hover:text-blue-300 font-mono text-xxs flex items-center space-x-1 transition" {
                                                    span { "+ Add Column Field" }
                                                }
                                        }

                                        // Column Entry Rows (Implicitly initialized with an auto-incrementing ID)
                                        div class="space-y-2" {
                                            // Fixed base Primary Key row display
                                            div class="flex gap-3 items-center opacity-50 bg-gray-900/20 p-2 rounded-lg border border-dashed border-gray-800" {
                                                div class="flex-1 text-xs font-mono text-gray-400 px-2" { "id" }
                                                div class="w-36 text-xs font-mono text-gray-400 px-2" { "BigInteger (PK / Auto)" }
                                                div class="w-8" {}
                                            }
                                            
                                            // Target track for interactive dynamic rows
                                            div id="dynamic-column-track" class="space-y-2" {}
                                        }
                                    }

                                    // Hidden JSON representation field synchronized automatically on submit
                                    input type="hidden" id="columns_data_input" name="columns_data" value="[]";

                                    // Footer Actions Container
                                    div class="pt-4 border-t border-gray-800 flex justify-end space-x-3 bg-gray-950" {
                                        button 
                                            type="button"
                                            onclick="document.getElementById('schema-modal').classList.add('hidden')"
                                            class="bg-gray-900 hover:bg-gray-800 border border-gray-800 text-gray-400 text-xs font-mono px-4 py-2 rounded-lg transition" {
                                            "Cancel"
                                        }
                                        button 
                                            type="submit"
                                            class="bg-blue-900/50 border border-blue-700/60 hover:bg-blue-800/50 text-blue-300 text-xs font-mono font-bold px-5 py-2 rounded-lg transition shadow-md" {
                                            "Build & Execute Schema"
                                        }
                                    }
                                }
                            }
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

                    div class="htmx-indicator-bar id-global-top-indicator" {}
                    main class="flex-1 overflow-y-auto py-4 px-0" {
                        div id="main-content" class="max-w-7xl mx-auto" {
                            (content)
                        }
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
