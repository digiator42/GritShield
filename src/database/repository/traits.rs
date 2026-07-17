use sea_orm::{
    ActiveModelBehavior, ActiveModelTrait, ColumnTrait, DatabaseConnection,
    EntityTrait, FromQueryResult, IntoActiveModel, ModelTrait, PaginatorTrait,
    PrimaryKeyTrait, QueryFilter, QueryOrder, QuerySelect, Select, TryIntoModel,
};
use sea_orm_migration::async_trait::async_trait;
use crate::database::repository::pagination::SortDirection;

use super::pagination::{Page, PageRequest, Sort};
use super::query_builder::QueryBuilder;

#[derive(Clone, Debug)]
pub struct GridColumn {
    pub name: &'static str,
    pub label: &'static str,
    pub is_editable: bool,
}

// =============================================================================
// REPOSITORY TRAIT
// =============================================================================

#[async_trait]
pub trait GritRepository {
    type Entity: EntityTrait<Model = Self::Model, Column = Self::Column>;
    type Model: ModelTrait + FromQueryResult + IntoActiveModel<Self::ActiveModel> + Send + Sync;
    type Column: ColumnTrait + sea_orm::Iterable + sea_orm::Iden + Clone + Send + Sync;
    type ActiveModel: ActiveModelTrait<Entity = Self::Entity>
        + ActiveModelBehavior
        + TryIntoModel<Self::Model>
        + ConvertFromModel<Self::Model>
        + Send
        + Sync;

    type Id: Into<<<Self::Entity as EntityTrait>::PrimaryKey as PrimaryKeyTrait>::ValueType>
        + Send
        + Sync
        + Clone
        + std::fmt::Debug;

    fn query(&self) -> QueryBuilder<Self::Entity> {
        QueryBuilder::new(self.get_db().clone())
    }

    // =========================================================================
    // STATIC HELPERS
    // =========================================================================

    fn find(&self) -> Select<Self::Entity> {
        Self::Entity::find()
    }

    async fn find_first(&self) -> Result<Option<Self::Model>, sea_orm::DbErr> {
        self.find()
            .order_by_asc(self.id_col())
            .one(self.get_db())
            .await
    }

    async fn find_last(&self) -> Result<Option<Self::Model>, sea_orm::DbErr> {
        self.find()
            .order_by_desc(self.id_col())
            .one(self.get_db())
            .await
    }

    async fn truncate(&self) -> Result<sea_orm::DeleteResult, sea_orm::DbErr> {
        Self::Entity::delete_many().exec(self.get_db()).await
    }

    fn id_col(&self) -> Self::Column {
        Self::id_column()
    }

    fn column_names(&self) -> Vec<String> {
        use sea_orm::{Iden, Iterable};

        <Self::Column as Iterable>::iter()
            .map(|col| col.to_string())
            .collect()
    }

    fn column_from_str(&self, name: &str) -> Option<Self::Column> {
        use sea_orm::{Iden, Iterable};

        <Self::Column as Iterable>::iter().find(|col| col.to_string() == name)
    }

    // =========================================================================
    // COMMON CRUD
    // =========================================================================

    async fn find_by_id(&self, id: Self::Id) -> Result<Option<Self::Model>, sea_orm::DbErr> {
        Self::Entity::find_by_id(id).one(self.get_db()).await
    }

    async fn delete_by_id(
        &self,
        id: Self::Id,
        user_id: Option<&str>,
    ) -> Result<sea_orm::DeleteResult, sea_orm::DbErr>;

    // ========== PAgination ============

    /// Fetch a page of results (0-based page index).
    async fn page(&self, page: u64, page_size: u64) -> Result<Vec<Self::Model>, sea_orm::DbErr> {
        self.find()
            .order_by_desc(self.id_col())
            .paginate(self.get_db(), page_size)
            .fetch_page(page)
            .await
    }

    // =========================================================================
    // CORE DATABASE ACCESS
    // =========================================================================

    fn get_db(&self) -> &DatabaseConnection;

    fn table_name(&self) -> String {
        use sea_orm::EntityName;
        <Self::Entity as Default>::default()
            .table_name()
            .to_string()
    }

    /// Declares the display order, names, labels, and edit permissions for spreadsheet columns
    fn grid_columns(&self) -> Vec<GridColumn> {
        vec![]
    }

    async fn search_admin_fields(&self, text: &str) -> Result<Vec<Self::Model>, sea_orm::DbErr>;

    /// Pulls a field's string representation dynamically out of a Model instance safely
    fn get_field_as_string(&self, _model: &Self::Model, _column_name: &str) -> String {
        String::new()
    }

    /// Insert an audit log entrDefault implementation does nothing.
    async fn audit_log(
        &self,
        _table_name: &str,
        _record_id: &str,
        _action: &str,
        _old_values: Option<serde_json::Value>,
        _new_values: Option<serde_json::Value>,
        _user_id: Option<&str>,
    ) -> Result<(), sea_orm::DbErr> {
        // default no-op – override in macro
        Ok(())
    }

    /// Performs updates directly onto records dynamically by string field names
    async fn update_column_value(
        &self,
        _id: Self::Id,
        _column_name: &str,
        _value: String,
        _user_id: Option<&str>,
    ) -> Result<Self::Model, sea_orm::DbErr> {
        Err(sea_orm::DbErr::Custom(
            "Dynamic update not implemented for this repository".to_string(),
        ))
    }

    // =========================================================================
    // COLUMN MAPPING (for dynamic queries)
    // =========================================================================

    fn id_column() -> Self::Column;
    fn email_column() -> Option<Self::Column> {
        None
    }
    fn created_at_column() -> Option<Self::Column> {
        None
    }
    fn updated_at_column() -> Option<Self::Column> {
        None
    }

    // =========================================================================
    // BASIC CRUD OPERATIONS (JPA Style)
    // =========================================================================

    async fn find_all(&self) -> Result<Vec<Self::Model>, sea_orm::DbErr> {
        Self::Entity::find().all(self.get_db()).await
    }

    async fn find_all_sorted(&self, sorts: Vec<Sort>) -> Result<Vec<Self::Model>, sea_orm::DbErr> {
        let mut query = Self::Entity::find();
        for sort in sorts {
            query = match sort.direction {
                SortDirection::Asc => query.order_by_asc(Self::get_column(&sort.field)?),
                SortDirection::Desc => query.order_by_desc(Self::get_column(&sort.field)?),
            };
        }
        query.all(self.get_db()).await
    }

    fn get_column(name: &str) -> Result<Self::Column, sea_orm::DbErr> {
        Err(sea_orm::DbErr::Custom(format!(
            "Column '{}' not found",
            name
        )))
    }

    async fn find_all_paginated(
        &self,
        page_request: PageRequest,
    ) -> Result<Page<Self::Model>, sea_orm::DbErr> {
        let mut query = Self::Entity::find();

        for sort in &page_request.sort {
            let column = Self::get_column(&sort.field)?;
            query = match sort.direction {
                SortDirection::Asc => query.order_by_asc(column),
                SortDirection::Desc => query.order_by_desc(column),
            };
        }

        let total = query.clone().count(self.get_db()).await?;

        let content = query
            .limit(page_request.limit())
            .offset(page_request.offset())
            .all(self.get_db())
            .await?;

        Ok(Page::new(content, total, &page_request))
    }

    // =========================================================================
    // FIND BY FIELD (Dynamic Field Queries)
    // =========================================================================

    async fn find_by_field<F>(
        &self,
        column: Self::Column,
        value: F,
    ) -> Result<Vec<Self::Model>, sea_orm::DbErr>
    where
        F: Into<sea_orm::Value> + Send + Sync,
    {
        Self::Entity::find()
            .filter(column.eq(value))
            .all(self.get_db())
            .await
    }

    async fn find_one_by_field<F>(
        &self,
        column: Self::Column,
        value: F,
    ) -> Result<Option<Self::Model>, sea_orm::DbErr>
    where
        F: Into<sea_orm::Value> + Send + Sync,
    {
        Self::Entity::find()
            .filter(column.eq(value))
            .one(self.get_db())
            .await
    }

    async fn find_by_field_contains(
        &self,
        column: Self::Column,
        value: &str,
    ) -> Result<Vec<Self::Model>, sea_orm::DbErr> {
        Self::Entity::find()
            .filter(column.contains(value))
            .all(self.get_db())
            .await
    }

    // =========================================================================
    // SAVE / UPDATE OPERATIONS (JPA Style)
    // =========================================================================

    async fn save(&self, model: Self::Model) -> Result<Self::Model, sea_orm::DbErr> {
        let active_model = <Self::ActiveModel as ConvertFromModel<Self::Model>>::from_model(model);
        let inserted_active = active_model.insert(self.get_db()).await?;
        inserted_active.try_into_model().map_err(|_| {
            sea_orm::DbErr::Custom("Failed to convert ActiveModel to Model".to_string())
        })
    }

    async fn save_all(&self, models: Vec<Self::Model>) -> Result<Vec<Self::Model>, sea_orm::DbErr> {
        let mut results = Vec::new();
        for model in models {
            results.push(self.save(model).await?);
        }
        Ok(results)
    }

    async fn update(&self, model: Self::Model) -> Result<Self::Model, sea_orm::DbErr> {
        let active_model = <Self::ActiveModel as ConvertFromModel<Self::Model>>::from_model(model);
        let updated_active = active_model.update(self.get_db()).await?;
        updated_active.try_into_model().map_err(|_| {
            sea_orm::DbErr::Custom("Failed to convert ActiveModel to Model".to_string())
        })
    }

    // =========================================================================
    // DELETE OPERATIONS (JPA Style)
    // =========================================================================

    async fn delete(&self, model: &Self::Model) -> Result<u64, sea_orm::DbErr> {
        let active_model =
            <Self::ActiveModel as ConvertFromModel<Self::Model>>::from_model(model.clone());
        let res = active_model.delete(self.get_db()).await?;
        Ok(res.rows_affected)
    }

    async fn delete_all(&self) -> Result<u64, sea_orm::DbErr> {
        let res = Self::Entity::delete_many().exec(self.get_db()).await?;
        Ok(res.rows_affected)
    }

    async fn delete_by_field<F>(
        &self,
        column: Self::Column,
        value: F,
    ) -> Result<u64, sea_orm::DbErr>
    where
        F: Into<sea_orm::Value> + Send + Sync,
    {
        let res = Self::Entity::delete_many()
            .filter(column.eq(value))
            .exec(self.get_db())
            .await?;
        Ok(res.rows_affected)
    }

    // =========================================================================
    // COUNT & EXISTENCE METHODS
    // =========================================================================

    async fn total_count(&self) -> Result<u64, sea_orm::DbErr> {
        Self::Entity::find().count(self.get_db()).await
    }

    async fn count_by_field<F>(&self, column: Self::Column, value: F) -> Result<u64, sea_orm::DbErr>
    where
        F: Into<sea_orm::Value> + Send + Sync,
    {
        Self::Entity::find()
            .filter(column.eq(value))
            .count(self.get_db())
            .await
    }

    async fn global_search(
        &self,
        query: &str,
        columns: Vec<Self::Column>,
    ) -> Result<Vec<Self::Model>, sea_orm::DbErr> {
        let search_query = Self::Entity::find();
        let mut condition = sea_orm::Condition::any();
        for column in columns {
            condition = condition.add(column.contains(query));
        }
        search_query.filter(condition).all(self.get_db()).await
    }
}

pub trait ConvertFromModel<M> {
    fn from_model(model: M) -> Self;
}
