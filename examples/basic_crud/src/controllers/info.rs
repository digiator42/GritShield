use gritshield::deps::sea_orm::{
    ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QuerySelect,
};
use gritshield::{database::repository::GritRepository, prelude::*};

use crate::{
    models::{post, user},
    repositories::user::UserRepository,
};

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

        Response::json(200, &repo_user_with_posts)
    }
}
