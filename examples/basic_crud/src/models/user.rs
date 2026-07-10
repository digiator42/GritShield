// src/models/user.rs
use chrono::NaiveDateTime;
use gritshield::{GritModel, GritRelation};
use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize, GritModel)]
#[sea_orm(table_name = "users")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub email: String,
    pub username: String,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation, GritRelation)]
#[grit(table = "users")]
pub enum Relation {
    #[sea_orm(has_many = "super::post::Entity")]
    Post,
    #[sea_orm(has_many = "super::comment::Entity")]
    Comment,
    #[sea_orm(
        has_many = "super::follower::Entity",
        from = "Column::Id",
        to = "super::follower::Column::FollowerId"
    )]
    Following,  // Users I am following
    #[sea_orm(
        has_many = "super::follower::Entity",
        from = "Column::Id",
        to = "super::follower::Column::FollowedId"
    )]
    Followers,  // Users following me
}

impl Related<super::post::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Post.def()
    }
}

impl Related<super::comment::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Comment.def()
    }
}

impl Related<super::follower::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Following.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

// Helper methods for user repository
impl Model {
    pub fn followers_count(&self) -> i64 {
        // This would be calculated in the repository
        0
    }
    
    pub fn following_count(&self) -> i64 {
        0
    }
}