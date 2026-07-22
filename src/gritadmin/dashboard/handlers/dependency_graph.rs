use crate::core::ioc::AutoWire;
use crate::http::response::Response;
use crate::routing::engine::RequestContext;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use maud::{html, Markup, PreEscaped};

pub async fn handle_topology_dashboard(_ctx: RequestContext) -> Response {
    let dot_schema = AutoWire::export_dot();
    let markup = render_topology_graph(&dot_schema);
    Response::ok(markup.into_string())
}

pub fn render_topology_graph(dot_schema: &str) -> Markup {
    let base64_dot = BASE64.encode(dot_schema);

    // Prepare single JS string with base64 embedded to avoid macro syntax collisions
    let js_code = format!(
        "(() => {{\nconst B64_DOT = '{}';\n{}\n}})();",
        base64_dot,
        r#"
let panZoomInstance = null;

function renderGritGraph() {
    const viewport = document.getElementById("graph-viewport");
    if (!viewport) return;

    if (typeof Viz === "undefined") {
        console.error("Viz.js is missing from <head>");
        viewport.innerHTML = '<div class="text-red-400 font-mono text-xs">Viz.js library missing.</div>';
        return;
    }

    try {
        const rawDot = atob(B64_DOT);
        
        const viz = new Viz();
        viz.renderSVGElement(rawDot)
            .then(function(element) {
                viewport.innerHTML = "";
                
                element.id = "grit-svg-graph";
                element.style.width = "100%";
                element.style.height = "100%";
                element.style.minHeight = "480px";
                viewport.appendChild(element);

                const texts = element.querySelectorAll("text");
                texts.forEach(t => {
                    const fc = t.getAttribute("fill");
                    if (fc) t.style.fill = fc;
                });

                if (panZoomInstance) {
                    panZoomInstance.destroy();
                }

                if (typeof svgPanZoom !== "undefined") {
                    panZoomInstance = svgPanZoom('#grit-svg-graph', {
                        zoomEnabled: true,
                        controlIconsEnabled: false,
                        fit: true,
                        center: true,
                        minZoom: 0.2,
                        maxZoom: 10
                    });
                }
            })
            .catch(function(error) {
                console.error("Viz.js Engine Error:", error);
                viewport.innerHTML = '<div class="text-red-400 font-mono text-xs">Render error: ' + error.message + '</div>';
            });
    } catch(e) {
        console.error("Viz Initialization / Base64 Decode Error:", e);
        viewport.innerHTML = '<div class="text-red-400 font-mono text-xs">Failed to initialize engine.</div>';
    }
}

// Attach control handlers safely to button IDs or keep them local to the IIFE
window.zoomInGritGraph = function() {
    if (panZoomInstance) panZoomInstance.zoomIn();
};

window.zoomOutGritGraph = function() {
    if (panZoomInstance) panZoomInstance.zoomOut();
};

window.resetGritGraph = function() {
    if (panZoomInstance) {
        panZoomInstance.resetZoom();
        panZoomInstance.center();
    }
};

window.toggleFullScreenGritGraph = function() {
    const viewport = document.getElementById("graph-viewport");
    if (!viewport) return;

    if (!document.fullscreenElement) {
        viewport.requestFullscreen().then(() => {
            if (panZoomInstance) {
                panZoomInstance.resize();
                panZoomInstance.fit();
                panZoomInstance.center();
            }
        }).catch(err => console.error(err));
    } else {
        document.exitFullscreen().then(() => {
            if (panZoomInstance) {
                panZoomInstance.resize();
                panZoomInstance.fit();
                panZoomInstance.center();
            }
        });
    }
};

window.renderGritGraph = renderGritGraph;

renderGritGraph();
"#
    );

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

                // Toolbar Actions & Interactive Controls
                div class="flex items-center justify-between text-xxs font-mono text-gray-400 border-b border-gray-900 pb-3" {
                    div class="flex items-center gap-2" {
                        span class="w-2 h-2 rounded-full bg-emerald-500 animate-pulse" {}
                        span class="text-gray-300 font-bold" { "Graph Engine: Graphviz WASM" }
                    }

                    // Interactive Zoom & Fullscreen Controls
                    div class="flex items-center gap-1.5" {
                        button
                            onclick="zoomInGritGraph()"
                            class="px-2 py-1 bg-gray-900 hover:bg-gray-800 border border-gray-800 rounded text-gray-300 transition" {
                            "➕ Zoom In"
                        }
                        button
                            onclick="zoomOutGritGraph()"
                            class="px-2 py-1 bg-gray-900 hover:bg-gray-800 border border-gray-800 rounded text-gray-300 transition" {
                            "➖ Zoom Out"
                        }
                        button
                            onclick="resetGritGraph()"
                            class="px-2 py-1 bg-gray-900 hover:bg-gray-800 border border-gray-800 rounded text-gray-300 transition" {
                            "🎯 Reset"
                        }
                        button
                            onclick="toggleFullScreenGritGraph()"
                            class="px-2 py-1 bg-purple-950/60 hover:bg-purple-900/60 border border-purple-800/80 rounded text-purple-300 transition font-bold" {
                            "⛶ Fullscreen"
                        }
                        button
                            onclick="renderGritGraph()"
                            class="px-2 py-1 bg-gray-900 hover:bg-gray-800 border border-gray-800 rounded text-gray-300 transition" {
                            "🔄 Refresh"
                        }
                    }
                }

                // Interactive Render Canvas Target
                div id="graph-viewport" class="w-full min-h-[500px] flex items-center justify-center bg-gray-900/30 rounded-lg border border-gray-850 p-2 overflow-hidden relative" {
                    div class="flex flex-col items-center gap-2 text-gray-500 font-mono text-xs animate-pulse" id="graph-loading" {
                        span { "⚡ Compiling Graphviz Vector Layout..." }
                    }
                }
            }
        }

        // Output raw JS directly without macro interpolation
        script {
            (PreEscaped(js_code))
        }
    }
}
