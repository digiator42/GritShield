// src/models/followers.rs
use chrono::NaiveDateTime;
use gritshield::{GritModel, GritRelation};
use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize, GritModel)]
#[sea_orm(table_name = "followers")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub follower_id: i64, // The user who is following
    pub followed_id: i64, // The user being followed
    pub created_at: NaiveDateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation, GritRelation)]
#[grit(table = "followers")]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::user::Entity",
        from = "Column::FollowerId",
        to = "super::user::Column::Id"
    )]
    Follower,
    #[sea_orm(
        belongs_to = "super::user::Entity",
        from = "Column::FollowedId",
        to = "super::user::Column::Id"
    )]
    Followed,
}

impl Related<super::user::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Follower.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
