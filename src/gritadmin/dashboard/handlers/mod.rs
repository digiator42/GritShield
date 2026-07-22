pub mod list;
pub mod search;
pub mod export_csv;
pub mod custom_action;
pub mod create_table;
pub mod alter_table;
pub mod metrics;
pub mod crud_ops;
pub mod rbac_graph;
pub mod dependency_graph;
pub mod jobs_queue_graph;
pub mod record_details;

// Re-export all handlers
pub use list::handle_list;
pub use search::handle_search;
pub use crud_ops::{handle_delete, handle_patch, handle_bulk_delete, handle_bulk_create, handle_bulk_create_modal};
pub use record_details::handle_detail;
pub use rbac_graph::handle_rbac_dashboard;
pub use dependency_graph::handle_topology_dashboard;
pub use jobs_queue_graph::handle_events_jobs_dashboard;
pub use export_csv::handle_export;
pub use custom_action::handle_custom_action;
pub use search::handle_custom_search_viewer;
pub use metrics::{handle_dashboard, admin_metrics_api_handler, admin_metrics_html_handler, admin_security_matrix_view_handler};
pub use create_table::handle_create_table_dynamic;
pub use alter_table::{handle_append_table_column, alter_table_add_column_handler};
pub use search::handle_search_palette;