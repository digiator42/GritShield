use gritshield::GritAdmin;

#[derive(Clone, GritAdmin)]
#[repository(
    searchable = ["id", "post_id", "created_at", "content"],
    grid_columns = ["id", "post_id", "content", "created_at"],
)]
pub struct CommentRepository {
    pub db: sea_orm::DatabaseConnection,
}
