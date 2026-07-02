use gritshield::GritAdmin;

#[derive(Clone, GritAdmin)]
#[repository(
    searchable = ["id", "user_id", "content"],
    grid_columns = ["id", "user_id", "content", "created_at"],
    read_only = ["created_at"],
)]
pub struct PostRepository {
    pub db: sea_orm::DatabaseConnection,
}
