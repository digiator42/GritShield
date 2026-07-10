use gritshield::GritAdmin;

#[derive(GritAdmin)]
#[repository(
    searchable = ["follower_id", "followed_id"],
    grid_columns = ["id", "follower_id", "followed_id", "created_at"],
    read_only = ["id", "created_at"],
)]
pub struct FollowerRepository {
    pub db: sea_orm::DatabaseConnection,
}