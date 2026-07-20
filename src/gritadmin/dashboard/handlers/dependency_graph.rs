use crate::core::ioc::AutoWire;
use crate::http::response::Response;
use crate::routing::engine::RequestContext;
use maud::{html, Markup, PreEscaped};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};

/// Renders the DI Component & Dependency Topology Graph Dashboard
pub async fn handle_topology_dashboard(_ctx: RequestContext) -> Response {
    // Export the live Graphviz DOT schema directly from the DI inventory
    let dot_schema = AutoWire::export_dot();

    // Render Maud layout
    let markup = render_topology_graph(&dot_schema);
    Response::ok(markup.into_string())
}

pub fn render_topology_graph(dot_schema: &str) -> Markup {
    // Encode the DOT string to Base64 to bypass all escaping/HTML parsing issues completely!
    let base64_dot = BASE64.encode(dot_schema);

    html! {
        div class="space-y-6 p-6 animate-slide-in" id="topology-graph-panel" {

            // Header Bar
            div class="flex items-center justify-between border-b border-gray-800 pb-4" {
                div {
                    h2 class="text-sm font-bold font-mono text-gray-200" { "🕸️ Service & Component Topology" }
                    p class="text-xxs font-mono text-gray-500 mt-0.5" {
                        "Live boot-time dependency graph collected across the binary inventory."
                    }
                }
                div class="flex items-center gap-2" {
                    span class="px-2.5 py-1 bg-purple-950/40 border border-purple-800/80 text-purple-400 font-mono text-xxs rounded-full font-bold" {
                        "AUTOWIRE GRAPH ACTIVE"
                    }
                }
            }

            // Visualizer Container
            div class="p-4 bg-gray-950 border border-gray-800 rounded-xl space-y-4 shadow-xl" {

                // Toolbar Actions
                div class="flex items-center justify-between text-xxs font-mono text-gray-400 border-b border-gray-900 pb-3" {
                    div class="flex items-center gap-2" {
                        span class="w-2 h-2 rounded-full bg-emerald-500 animate-pulse" {}
                        span class="text-gray-300 font-bold" { "Graph Engine: Graphviz WASM" }
                    }
                    button
                        onclick="renderGritGraph()"
                        class="px-2 py-1 bg-gray-900 hover:bg-gray-800 border border-gray-800 rounded text-gray-300 transition" {
                        "🔄 Refresh Matrix"
                    }
                }

                // Interactive Render Canvas Target
                div id="graph-viewport" class="w-full min-h-[450px] flex items-center justify-center bg-gray-900/30 rounded-lg border border-gray-850 p-6 overflow-auto" {
                    div class="flex flex-col items-center gap-2 text-gray-500 font-mono text-xs animate-pulse" id="graph-loading" {
                        span { "⚡ Compiling Graphviz Vector Layout..." }
                    }
                }
            }
        }

        // Script block that decodes the Base64 payload in JavaScript
        script {
            (PreEscaped(format!(r#"
                function renderGritGraph() {{
                    const b64Dot = "{}";
                    const viewport = document.getElementById("graph-viewport");
                    if (!viewport) return;

                    if (typeof Viz === "undefined") {{
                        console.error("Viz.js is missing from <head>");
                        viewport.innerHTML = `<div class="text-red-400 font-mono text-xs">Viz.js library missing.</div>`;
                        return;
                    }}

                    try {{
                        // Decode Base64 string back into clean DOT graph syntax
                        const rawDot = atob(b64Dot);
                        
                        const viz = new Viz();
                        viz.renderSVGElement(rawDot)
                            .then(function(element) {{
                                viewport.innerHTML = "";
                                viewport.appendChild(element);
                                element.setAttribute("width", "100%");
                                element.setAttribute("height", "100%");
                                element.style.maxHeight = "600px";
                            }})
                            .catch(function(error) {{
                                console.error("Viz.js Engine Error:", error);
                                viewport.innerHTML = `<div class="text-red-400 font-mono text-xs">Render error: ${{error.message}}</div>`;
                            }});
                    }} catch(e) {{
                        console.error("Viz Initialization / Base64 Decode Error:", e);
                        viewport.innerHTML = `<div class="text-red-400 font-mono text-xs">Failed to initialize engine.</div>`;
                    }}
                }}

                renderGritGraph();
            "#, base64_dot)))
        }
    }
}
