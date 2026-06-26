use gritshield::GritRepository;
use sea_orm::DatabaseConnection;

#[derive(GritRepository)]
#[repository(
    searchable = ["username", "email", "updated_at"],
    grid_columns = ["id", "username", "email", "updated_at"],
    read_only = ["updated_at"]
)]
pub struct UserRepository {
    pub db: DatabaseConnection,
}
