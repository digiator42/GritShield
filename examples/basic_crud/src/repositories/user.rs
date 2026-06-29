use gritshield::{GritAdmin, GritRepository};

#[derive(GritRepository, GritAdmin)]
#[repository(
    entity = "crate::models::user",
    searchable = ["id", "email"],
    grid_columns = ["id", "email", "created_at", "updated_at"],
    read_only = ["updated_at"],
    has_many = ["posts", "comments"]
)]
pub struct UserRepository {
    pub db: sea_orm::DatabaseConnection,
}
