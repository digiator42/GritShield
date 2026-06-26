use gritshield::GritRepository;

#[derive(GritRepository)]
#[repository(
    entity = crate::models::user,   // add this
    searchable = ["email", "username"],             // Fields included in query searches
    grid_columns = ["id", "email", "username", "updated_at"],     // Layout spreadsheet columns
    read_only = ["updated_at"]                              // Disallow dynamic cell edits on 'id'
)]
pub struct UserRepository {
    pub db: sea_orm::DatabaseConnection,
}
