use gritshield::GritRepository;
use sea_orm::DatabaseConnection;

#[derive(GritRepository)]
#[repository(admin_searchable = ["email", "username"])]
pub struct UserRepository {
    pub db: DatabaseConnection,
}
