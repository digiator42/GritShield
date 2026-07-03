use chrono::NaiveDateTime;
use gritshield_macros::{GritModel, GritRelation};
use sea_orm::entity::prelude::*;
use sea_orm::prelude::Json;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq, Deserialize, Serialize)]
#[sea_orm(table_name = "audit_logs")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub table_name: String,
    pub record_id: String,
    pub action: String,
    pub old_values: Option<Json>,
    pub new_values: Option<Json>,
    pub user_id: Option<String>,
    pub timestamp: NaiveDateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
