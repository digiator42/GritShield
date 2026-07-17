use gritshield::action;
use gritshield::prelude::*;
use sea_orm::ConnectionTrait;

/// Publish selected posts (sets status to "published")
#[action(
    table = "posts",
    label = "publish",
    icon = "📢",
    color = "text-emerald-400"
)]
async fn publish_posts(ctx: RequestContext) -> Response {
    let ids = ctx.form.fields.get("ids").unwrap();
    let ids: Vec<&str> = ids.as_str().split(',').filter(|s| !s.is_empty()).collect();

    let db = match ctx.db.clone() {
        Some(d) => d,
        None => return Response::bad_request("Database connection missing"),
    };

    // Update posts with status = "published"
    for id in &ids {
        let id = match id.parse::<i64>() {
            Ok(i) => i,
            Err(_) => continue,
        };

        let sql = format!("UPDATE posts SET status = 'published' WHERE id = {}", id);
        let stmt = sea_orm::Statement::from_string(sea_orm::DatabaseBackend::Sqlite, sql);
        let _ = db.as_ref().execute(stmt).await;
    }

    let message = format!("✅ {} posts published!", ids.len());

    // Return the updated table body (re-fetch from server)
    // Or just show a toast message
    let response = html! {
        div class="text-green-400 text-sm font-mono p-2" {
            (message)
        }
    };

    Response::ok(response.into_string())
}

/// Archive selected posts (soft delete or move to archive)
#[action(
    table = "posts",
    label = "archive",
    icon = "📦",
    color = "text-amber-400"
)]
async fn archive_posts(ctx: RequestContext) -> Response {
    let ids = ctx.form.fields.get("ids").unwrap();
    let ids: Vec<&str> = ids.as_str().split(',').filter(|s| !s.is_empty()).collect();

    let db = match ctx.db.clone() {
        Some(d) => d,
        None => return Response::bad_request("Database connection missing"),
    };

    // Archive posts (soft delete or move to archived)
    for id in &ids {
        let id = match id.parse::<i64>() {
            Ok(i) => i,
            Err(_) => continue,
        };

        let sql = format!("UPDATE posts SET status = 'archived' WHERE id = {}", id);
        let stmt = sea_orm::Statement::from_string(sea_orm::DatabaseBackend::Sqlite, sql);
        let _ = db.as_ref().execute(stmt).await;
    }

    let message = format!("📦 {} posts archived!", ids.len());

    let response = html! {
        div class="text-amber-400 text-sm font-mono p-2" {
            (message)
        }
    };

    Response::ok(response.into_string())
}

/// Delete selected posts permanently
#[action(table = "posts", label = "delete", icon = "🗑️", color = "text-red-400")]
async fn delete_posts(ctx: RequestContext) -> Response {
    let ids = ctx.form.fields.get("ids").unwrap();
    let ids: Vec<&str> = ids.as_str().split(',').filter(|s| !s.is_empty()).collect();

    let db = match ctx.db.clone() {
        Some(d) => d,
        None => return Response::bad_request("Database connection missing"),
    };

    for id in &ids {
        let id = match id.parse::<i64>() {
            Ok(i) => i,
            Err(_) => continue,
        };

        let sql = format!("DELETE FROM posts WHERE id = {}", id);
        let stmt = sea_orm::Statement::from_string(sea_orm::DatabaseBackend::Sqlite, sql);
        let _ = db.as_ref().execute(stmt).await;
    }

    let message = format!("🗑️ {} posts permanently deleted!", ids.len());

    let response = html! {
        div class="text-red-400 text-sm font-mono p-2" {
            (message)
        }
    };

    Response::ok(response.into_string())
}
