use gritshield::{database::repository::GritRepository, GritRepository};
use sea_orm::DatabaseConnection;

#[derive(GritRepository)]
#[repository(admin_searchable = ["email", "username"])]
pub struct UserRepository {
    pub db: DatabaseConnection,
}

impl UserRepository {
    pub async fn global_search_paginated(
        &self,
        query: &str,
        searchable_columns: Vec<crate::models::user::Column>,
        page: u64,
        page_size: u64,
    ) -> Result<(Vec<crate::models::user::Model>, u64, u64), sea_orm::DbErr> {
        use sea_orm::sea_query::Condition;
        use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder};

        // 1. Initialize a dynamic, empty OR group condition matrix
        let mut condition = Condition::any();

        // 2. Loop through and push each column filter statement directly into the engine
        for col in searchable_columns {
            condition = condition.add(col.like(format!("%{}%", query)));
        }

        let paginator = <UserRepository as GritRepository>::Entity::find()
            .filter(condition)
            .order_by_desc(<UserRepository as GritRepository>::id_column())
            .paginate(&self.db, page_size); // Note: pass &self.db to prevent ownership moves if needed

        let total_pages = paginator.num_pages().await?;
        let total_count = paginator.num_items().await?;
        let users = paginator.fetch_page(page).await?;

        Ok((users, total_count, total_pages))
    }
}
