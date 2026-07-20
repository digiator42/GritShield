use crate::prelude::*;
use crate::routing::engine::route::CapabilityRegistration;
use maud::{html, Markup};
use std::collections::HashMap;

/// High-fidelity structural metadata for RBAC transparency tracking
pub async fn handle_rbac_dashboard(ctx: RequestContext) -> Response {
    let inheritance = &ctx.role_inheritance;

    // 1. Collect declared capability configurations from the inventory engine
    let mut declared_caps = Vec::new();
    for cap in inventory::iter::<CapabilityRegistration> {
        let roles: Vec<String> = cap.allowed_roles.iter().map(|r| r.to_string()).collect();
        declared_caps.push((cap.name.to_string(), roles));
    }

    // 2. Collect automatically discovered endpoint routing profiles
    let mut route_capabilities = Vec::new();
    for route in crate::inventory::iter::<crate::routing::AutoRoute> {
        if !route.path.contains("/admin/") {
            let security_display = match (route.capabilities, route.required_role) {
                (Some(caps), _) => format!("{}", caps),
                (None, Some(role)) => format!("{}", role),
                (None, None) => "!RBAC".to_string(),
            };

            route_capabilities.push((
                format!("{:?}", route.method),
                route.path.to_string(),
                security_display,
            ));
        }
    }

    // Pass the declared_caps array down to your Maud render layout
    let markup = render_rbac_graph(inheritance, &route_capabilities, &declared_caps);
    Response::ok(markup.into_string())
}

pub fn render_rbac_graph(
    inheritance: &HashMap<String, Vec<String>>,
    route_caps: &[(String, String, String)],
    declared_caps: &[(String, Vec<String>)],
) -> Markup {
    html! {
        div class="space-y-6 p-6 animate-slide-in" id="rbac-graph-panel" {

            // Header Action Context block
            div class="flex items-center justify-between border-b border-gray-800 pb-4" {
                div {
                    h2 class="text-sm font-bold font-mono text-gray-200" { "🔍 Core Access Governance Matrix" }
                    p class="text-xxs font-mono text-gray-500 mt-0.5" { "Audit active compile-time structural fences and recursive runtime authorization lineages." }
                }
                span class="px-2.5 py-1 bg-blue-950/40 border border-blue-800/80 text-blue-400 font-mono text-xxs rounded-full font-bold animate-pulse" { "🛡️ RBAC ENGINE ACTIVE" }
            }

            // Dual Column Analytical Split
            div class="grid grid-cols-1 lg:grid-cols-3 gap-6" {

                // Left Column: Dynamic Role Inheritance Tree Layout (Spans 1 Column)
                div class="lg:col-span-1 p-4 bg-gray-950 border border-gray-800 rounded-xl space-y-4" {
                    div {
                        span class="text-xxs font-mono uppercase text-gray-400 tracking-wider block font-semibold" { "Dynamic Runtime Hierarchies" }
                        p class="text-[10px] font-mono text-gray-500 mt-0.5 leading-normal" { "Parent roles recursively inherit all downstream permissions down their respective lineage path branches." }
                    }

                    @if inheritance.is_empty() {
                        div class="p-4 bg-gray-900/20 border border-dashed border-gray-800 text-center rounded-lg text-xxs font-mono text-gray-500" {
                            "No dynamic role inheritance vectors configured at router boot initialization."
                        }
                    } @else {
                        div class="space-y-3 font-mono text-xxs" {
                            @for (parent, children) in inheritance {
                                div class="p-3 bg-gray-900/40 border border-gray-850 rounded-lg space-y-2" {
                                    div class="flex items-center gap-2" {
                                        span class="w-1.5 h-1.5 rounded-full bg-blue-500" {}
                                        span class="font-bold text-gray-200" { (parent) }
                                    }

                                    // Visual directional inheritance alignment
                                    div class="pl-4 border-l border-gray-800 space-y-1.5 pt-0.5" {
                                        @for child in children {
                                            div class="flex items-center gap-2 text-gray-400" {
                                                span class="text-gray-600 font-sans" { "└──" }
                                                span class="bg-gray-950 border border-gray-800 px-1.5 py-0.5 rounded text-[10px]" { (child) }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                // Right Column: Compile-Time Capability Router Endpoint Ledger (Spans 2 Columns)
                div class="lg:col-span-2 p-4 bg-gray-950 border border-gray-800 rounded-xl space-y-4" {
                    div {
                        span class="text-xxs font-mono uppercase text-gray-400 tracking-wider block font-semibold" { "Compile-Time Guard Matrix" }
                        p class="text-[10px] font-mono text-gray-500 mt-0.5 leading-normal" { "Verified endpoints compiled securely with hardcoded structural capability proofs." }
                    }

                    @if route_caps.is_empty() {
                        div class="p-4 bg-gray-900/20 border border-dashed border-gray-800 text-center rounded-lg text-xxs font-mono text-gray-500" {
                            "No active secured route instances discovered inside the framework telemetry index."
                        }
                    } @else {
                        div class="overflow-hidden border border-gray-850 rounded-lg" {
                            table class="w-full text-left font-mono text-xxs m-0 border-collapse" {
                                thead class="bg-gray-900 text-gray-400 uppercase tracking-wider" {
                                    tr {
                                        th class="p-2.5 font-semibold w-16" { "Verb" }
                                        th class="p-2.5 font-semibold" { "Route Context Namespace Path" }
                                        th class="p-2.5 font-semibold text-right" { "RBAC" }
                                    }
                                }
                                tbody class="divide-y divide-gray-850" {
                                    @for (method, path, target_rule) in route_caps {
                                        tr class="hover:bg-gray-900/40 transition" {
                                            td class="p-2.5" {
                                                @if method == "GET" {
                                                    span class="text-blue-400 bg-blue-950/30 px-1.5 py-0.5 rounded font-bold text-[9px]" { (method) }
                                                } @else if method == "POST" {
                                                    span class="text-emerald-400 bg-emerald-950/30 px-1.5 py-0.5 rounded font-bold text-[9px]" { (method) }
                                                } @else {
                                                    span class="text-amber-400 bg-amber-950/30 px-1.5 py-0.5 rounded font-bold text-[9px]" { (method) }
                                                }
                                            }
                                            td class="p-2.5 text-gray-300 font-medium" { (path) }
                                            td class="p-2.5 text-right font-bold" {
                                                @if target_rule.contains("Admin") {
                                                    span class="text-red-400 bg-red-950/20 border border-red-900/40 px-2 py-0.5 rounded text-[10px]" { (target_rule) }
                                                } @else {
                                                    span class="text-purple-400 bg-purple-950/20 border border-purple-900/40 px-2 py-0.5 rounded text-[10px]" { (target_rule) }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            // Add a block tracking Cap token values inside the UI panels
        div class="p-4 bg-gray-950 border border-gray-800 rounded-xl space-y-3" {
            span class="text-xxs font-mono uppercase text-gray-400 tracking-wider block font-semibold" { "Active Capability Rule Book Matrices" }

            div class="grid grid-cols-1 md:grid-cols-3 gap-3 font-mono text-xxs" {
                @for (cap_name, roles) in declared_caps {
                    div class="p-3 bg-gray-900/40 border border-gray-850 rounded-lg space-y-2" {
                        span class="text-purple-400 font-bold block" { "#[" (cap_name) "]" }
                        div class="flex flex-wrap gap-1.5" {
                            @for role in roles {
                                span class="bg-gray-950 border border-gray-800 text-gray-300 px-1.5 py-0.5 rounded text-[10px]" { (role) }
                            }
                        }
                    }
                }
            }
        }
        }
    }
}
