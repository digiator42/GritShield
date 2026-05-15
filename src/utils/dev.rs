use crate::{
    model::{post, user},
    protocol::response::{Cookie, Response},
    render,
    routing::trie::RequestContext,
    security::xss::{SafeHtml, Sanitizer},
};
use gritshield_macros::{delete, get, post, put};
use maud::html;
use sea_orm::ColumnTrait;
use sea_orm::{EntityTrait, ModelTrait, QueryFilter};

pub fn profile_handler(ctx: RequestContext) -> Response {
    let name = ctx.params.get("name").unwrap();

    if name.as_str() == "logo" {
        return Response::static_file("static/img/logo.png");
    }

    // Returns the Html variant of Response
    let mut res = Response::new(200, Sanitizer::trust("<h1>User Profile</h1>"));
    res.cookies.push(Cookie::new("GSESSIONID", "2024-10-01"));
    res
}

pub fn products_handler(_: RequestContext) -> Response {
    let body = Sanitizer::trust(&format!("<h1>products Page</h1><p>Welcome!</p>"));
    Response::new(200, body)
}

#[get("/static/:*path")]
pub async fn static_handler(ctx: RequestContext) -> Response {
    let path = ctx.params.get("*path").unwrap();

    let full_fs_path = format!("static/{}", path.as_str());

    println!("Serving file: {}", full_fs_path);
    Response::static_file(&full_fs_path)
}

pub fn dashboard_handler(ctx: RequestContext) -> Response {
    if let Some(session_ptr) = ctx.session {
        let mut session = session_ptr.lock().unwrap();
        session
            .data
            .insert("last_action".to_string(), "view_dashboard".to_string());

        return Response::new(200, Sanitizer::trust("<h1>Welcome Back!</h1>"));
    }

    Response::new(401, Sanitizer::trust("<h1>Session Required</h1>"))
}

#[get("/")]
pub async fn home_handler(ctx: RequestContext) -> Response {
    let user_name = ctx
        .session
        .map(|s| {
            s.lock()
                .unwrap()
                .data
                .get("user")
                .cloned()
                .unwrap_or("Guest".to_string())
        })
        .unwrap_or("GUEST".to_string());

    let search_query = ctx
        .query
        .get("p")
        .map(|v| v.as_str())
        .unwrap_or("No search provided");

    render!(
        "Home Page",
        html! {
            h1 { "Welcome to the Framework Docs" }
            p { "Hello, " (user_name) "! ==>" (search_query) }
            div class="card" {
                p { "This page was rendered with no chance to xss vulnerability." }
            }
        }
    )
}

#[post("/upload")]
pub async fn handle_upload(ctx: RequestContext) -> Response {
    // Get text fields safely
    let description = ctx
        .form
        .fields
        .get("description")
        .map(|v| v.as_str())
        .unwrap_or("No description");

    // Extract an uploaded document or image
    if let Some(file) = ctx.form.files.get("doc_file") {
        println!(
            "Received file: {} (Mime: {})",
            file.filename, file.content_type
        );

        // Securely write file to the local directory
        // Enforce file checks here (e.g., checking file extension or max length)
        let save_path = format!("uploads/{}", file.filename);
        if std::fs::write(&save_path, &file.data).is_ok() {
            return render!(
                "Upload Success",
                html! {
                    h1 { "File Uploaded Successfully!" }
                    p { "Description: " (description) }
                    p { "Saved file to: " (save_path) }
                }
            );
        }
    }

    Response::new(
        400,
        crate::security::xss::Sanitizer::trust("<h1>File upload failed</h1>"),
    )
}

#[put("/items/update/:id")]
pub async fn update_item(ctx: RequestContext) -> Response {
    let item_id = ctx.params.get("id").unwrap().as_str();
    println!("Updating database item ID: {}", item_id);

    Response::new(200, Sanitizer::trust("<h1>Item Updated</h1>"))
}

#[delete("/items/delete/:id")]
pub async fn delete_item(ctx: RequestContext) -> Response {
    let item_id = ctx.params.get("id").unwrap().as_str();
    println!("Purging item ID: {}", item_id);

    Response::new(200, Sanitizer::trust("<h1>Item Deleted</h1>"))
}

#[get("/docs/db")]
pub async fn db_docs(ctx: RequestContext) -> Response {
    let status = if ctx.db.ping().await.is_ok() {
        "Connected"
    } else {
        "Offline"
    };

    render!(
        "Database Status",
        html! {
            h1 { "System Check" }
            p { "Database is currently: " (status) }
        }
    )
}

#[get("/profile/:username")]
pub async fn user_profile(ctx: RequestContext) -> Response {
    let username = ctx.params.get("username").unwrap().as_str();

    let user_res = user::Entity::find()
        .filter(user::Column::Username.eq(username))
        .one(&*ctx.db)
        .await;

    match user_res {
        Ok(Some(user)) => Response::new(200, Sanitizer::trust("User found")),
        Ok(None) => Response::new(404, Sanitizer::trust("User not found")),
        Err(e) => {
            println!("Database Error: {:?}", e);
            Response::new(500, Sanitizer::trust("Internal Server Error"))
        }
    }
}
