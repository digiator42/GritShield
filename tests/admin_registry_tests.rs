//! The admin panel registers one route tree per model, and the grid, search box,
//! pager and delete/export buttons all build their own URLs from the *table
//! slug*. Those two things have to agree: the sidebar nav links to
//! `ModelMetadata::route_path`, while every in-page control links to
//! `/admin/{table_slug}/...`. When they disagree the panel loads and then 404s
//! on the first thing you click.
//!
//! These tests pin the invariant that keeps them equal. The derives below hard
//! code `crate::models::<slug>` and `crate::repositories::<slug>::<X>Repository`,
//! so the module layout has to mirror what a real application uses. The
//! `#[ctor]` startup hooks they expand to populate `ADMIN_REGISTRY` before the
//! first test runs.

#![cfg(feature = "admin")]

use gritshield::database::repository::registry::{ModelMetadata, ADMIN_REGISTRY};
use sea_orm::DatabaseConnection;

mod models {
    pub mod widget {
        use gritshield::{GritModel, GritRelation};
        use sea_orm::entity::prelude::*;
        use serde::{Deserialize, Serialize};

        #[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize, GritModel)]
        #[sea_orm(table_name = "widgets")]
        pub struct Model {
            #[sea_orm(primary_key)]
            pub id: i64,
            pub label: String,
        }

        #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation, GritRelation)]
        #[grit(table = "widgets")]
        pub enum Relation {}

        impl ActiveModelBehavior for ActiveModel {}
    }
}

mod repositories {
    pub mod widget {
        use gritshield::GritAdmin;

        #[derive(Clone, GritAdmin)]
        #[repository(searchable = ["label"], grid_columns = ["id", "label"])]
        pub struct WidgetRepository {
            pub db: super::super::DatabaseConnection,
        }
    }
}

fn metadata(table_name: &str) -> ModelMetadata {
    let registry = ADMIN_REGISTRY.lock().unwrap();
    registry
        .get(table_name)
        .unwrap_or_else(|| {
            panic!(
                "no admin model registered for `{table_name}`; registry holds {:?}",
                registry.keys()
            )
        })
        .clone()
}

#[test]
fn test_admin_routes_are_registered_under_the_table_slug() {
    let meta = metadata("widgets");

    assert_eq!(
        meta.table_slug, "widget",
        "slug is derived from the repository name, minus `Repository`, lowercased"
    );
    assert_eq!(
        meta.route_path, "/admin/widget",
        "route_path must be built from the slug, not the plural table name: \
         `widgets` would register /admin/widgets while the grid links to /admin/widget"
    );
}

#[test]
fn test_every_generated_sub_route_matches_the_route_path() {
    let meta = metadata("widgets");
    let slug = meta.route_path;

    // Exactly the paths `register_admin_routes` mounts off `route_path`...
    for suffix in [
        "/search",
        "/delete",
        "/update-cell",
        "/query-explorer",
        "/:id",
        "/bulk-delete",
        "/export",
    ] {
        // ...compared against what the handlers put in the rendered HTML.
        assert_eq!(
            format!("{}{}", slug, suffix),
            format!("/admin/{}{}", meta.table_slug, suffix),
            "`{slug}{suffix}` is mounted, but the panel requests a different URL"
        );
    }
}

#[test]
fn test_table_name_is_still_reported_separately_from_the_slug() {
    let meta = metadata("widgets");

    assert_eq!(
        meta.table_name, "widgets",
        "the table name stays plural -- audit logging and the metrics page label \
         columns with the real table name, and only the URL uses the slug"
    );
    assert_ne!(meta.table_name, meta.table_slug);
}