use sea_orm::{Database, DatabaseConnection, DbErr};

pub async fn connect(url: &str) -> Result<DatabaseConnection, DbErr> {
    println!("[GRITSHIELD] Connecting to database at {}...", url);
    Database::connect(url).await
}
