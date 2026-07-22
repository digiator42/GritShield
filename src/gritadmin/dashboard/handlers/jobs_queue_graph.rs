use crate::core::event_bus::EventBusGraph;
use crate::http::response::Response;
use crate::routing::engine::RequestContext;

pub async fn handle_events_jobs_dashboard(_ctx: RequestContext) -> Response {
    // Generate DOT schema for Jobs & Events
    let dot_schema = EventBusGraph::export_dot();
    
    // Reuse your existing Maud rendering layout!
    let markup = super::dependency_graph::render_topology_graph(&dot_schema);
    Response::ok(markup.into_string())
}