use gritshield::deps::sea_orm::{
    ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QuerySelect,
};
use gritshield::{database::repository::GritRepository, prelude::*};
use serde_json::Value;

use crate::models::comment;
use crate::repositories::post::PostRepository;
use crate::{
    models::{post, user},
    repositories::user::UserRepository,
};

use serde::{Deserialize, Serialize};

#[derive(GritSchema, Serialize, Deserialize)]
pub struct SwaggerTestData {
    pub email: String,
    pub name: String,
    pub age: Option<i32>,
}

pub struct ApiController;

#[controller("/api")]
impl ApiController {
    #[get("/info")]
    pub async fn system_info(ctx: RequestContext) -> Response {
        let db = ctx.db.as_deref().unwrap().clone();

        let user_repo = UserRepository { db: db.clone() };

        let sea_user_with_posts = user::Entity::find()
            .filter(user::Column::Email.eq("user_1@example.com"))
            .find_with_related(post::Entity)
            .all(&db)
            .await
            .unwrap();

        let repo_user_with_posts = user_repo
            .find_by_email("user_1@example.com")
            .with_posts()
            .with_comments()
            .await
            .unwrap();

        // let repo_user_query = user_repo
        //     .query()
        //     .where_gt(user::Column::Id, 3)
        //     .fetch()
        //     .await
        //     .unwrap();

        // =================== GritModel ====================
        let test_grit_model = user_repo
            .find_by_id_between(5, 6)
            .with_posts()
            .with_comments()
            .await
            .unwrap();

        let post_repo = PostRepository { db: db.clone() };

        let posts_with_comments = post_repo
            .find_by_id(2)
            .with_comments()
            // .with_users()
            .await
            .unwrap();

        // let posts_by_query = post_repo.search_admin_fields("03:15:07").await.unwrap();

        let user_tree = user_repo
            .find_by_email("user_1@example.com")
            // .with_comments_nested(|query| query.with_users())
            .await
            .unwrap();

        Response::json(200, &sea_user_with_posts)
    }

    // Use the body parameter in the route macro
    #[post("/swagger-body", body = SwaggerTestData)]
    pub async fn test_swagger_body(ctx: RequestContext) -> Response {
        // Parse the JSON body
        let data: Value = match ctx.json_body().await {
            Some(d) => d,
            None => Value::Null,
        };

        Response::ok(format!("Hello, {}!", data.get("name").unwrap()))
    }

    #[post("/swagger")]
    pub async fn test_swagger(ctx: RequestContext) -> Response {
        let db = ctx.db.as_deref().unwrap().clone();

        let data = ctx.form.fields;

        let user_repo = UserRepository { db: db.clone() };

        let sea_user_with_posts = user_repo
            .find_by_email(data["email"].as_str())
            .with_posts()
            .await
            .unwrap();

        Response::json(200, &sea_user_with_posts)
    }

    #[delete("/")]
    async fn hello(_: RequestContext) -> &'static str {
        "Hello, GritShield!"
    }
}
