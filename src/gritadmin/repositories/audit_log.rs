use crate::GritAdmin;

#[derive(GritAdmin)]
#[repository(
    entity = "crate::gritadmin::models::audit_log",
    searchable = ["table_name", "record_id", "action", "old_values", "new_values", "user_id", "timestamp"],
    read_only = ["all"]
)]
pub struct AuditLogRepository {
    pub db: sea_orm::DatabaseConnection,
}