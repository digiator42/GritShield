use gritshield::GritAdmin;

#[derive(GritAdmin)]
#[repository(
    searchable = ["username", "email", "created_at", "updated_at"],
    grid_columns = ["id", "username", "created_at", "updated_at"],
    read_only = ["created_at"],
)]
pub struct UserRepository {
    pub db: sea_orm::DatabaseConnection,
}
