use crate::core::event_bus::EventBusGraph;
use crate::http::response::Response;
use crate::routing::engine::RequestContext;

pub async fn handle_events_jobs_dashboard(ctx: RequestContext) -> Response {
    let is_htmx = ctx.req.has_header("hx-request");

    let dot_schema = EventBusGraph::export_dot();
    let markup = super::dependency_graph::render_topology_graph(&dot_schema);

    if is_htmx {
        Response::ok(markup.into_string())
    } else {
        crate::admin_shell("Jobs & Events Topology", markup, false)
    }
}
