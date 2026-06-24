use sea_orm::{
    ActiveModelBehavior, ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait,
    FromQueryResult, IntoActiveModel, LoaderTrait, ModelTrait, PaginatorTrait, PrimaryKeyTrait,
    QueryFilter, QueryOrder, QueryOrder as QueryOrderTrait, QueryResult, QuerySelect, Select,
    SelectTwoMany, TryIntoModel,
};
use sea_orm_migration::async_trait::async_trait;

// =============================================================================
// PAGINATION & SORTING
// =============================================================================

#[derive(Debug, Clone)]
pub struct PageRequest {
    pub page: u64,
    pub size: u64,
    pub sort: Vec<Sort>,
}

impl PageRequest {
    pub fn new(page: u64, size: u64) -> Self {
        Self {
            page,
            size,
            sort: Vec::new(),
        }
    }

    pub fn with_sort(mut self, sort: Vec<Sort>) -> Self {
        self.sort = sort;
        self
    }

    pub fn offset(&self) -> u64 {
        self.page * self.size
    }

    pub fn limit(&self) -> u64 {
        self.size
    }
}

impl Default for PageRequest {
    fn default() -> Self {
        Self {
            page: 0,
            size: 20,
            sort: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Sort {
    pub field: String,
    pub direction: SortDirection,
}

impl Sort {
    pub fn asc(field: String) -> Self {
        Self {
            field,
            direction: SortDirection::Asc,
        }
    }

    pub fn desc(field: String) -> Self {
        Self {
            field,
            direction: SortDirection::Desc,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SortDirection {
    Asc,
    Desc,
}

#[derive(Debug, Clone)]
pub struct Page<T> {
    pub content: Vec<T>,
    pub total_elements: u64,
    pub total_pages: u64,
    pub size: u64,
    pub number: u64,
}

impl<T> Page<T> {
    pub fn new(content: Vec<T>, total_elements: u64, page_request: &PageRequest) -> Self {
        let total_pages = if page_request.size > 0 {
            (total_elements + page_request.size - 1) / page_request.size
        } else {
            0
        };

        Self {
            content,
            total_elements,
            total_pages,
            size: page_request.size,
            number: page_request.page,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.content.is_empty()
    }

    pub fn has_content(&self) -> bool {
        !self.is_empty()
    }
}

// =============================================================================
// SPECIFICATION PATTERN (Simplified)
// =============================================================================

pub trait Specification<E: EntityTrait> {
    fn apply(&self, query: Select<E>) -> Select<E>;
}

// =============================================================================
// REPOSITORY TRAIT
// =============================================================================

#[async_trait]
pub trait GritRepository {
    // Core Sea-ORM Entity Type Mapping Relationships
    type Entity: EntityTrait<Model = Self::Model, Column = Self::Column>;
    type Model: ModelTrait + FromQueryResult + IntoActiveModel<Self::ActiveModel> + Send + Sync;
    type Column: ColumnTrait + Send + Sync;
    type ActiveModel: ActiveModelTrait<Entity = Self::Entity>
        + ActiveModelBehavior
        + TryIntoModel<Self::Model>
        + ConvertFromModel<Self::Model>
        + Send
        + Sync;

    // Bind Id directly to the actual underlying Sea-ORM Primary Key value type
    type Id: Into<<<Self::Entity as EntityTrait>::PrimaryKey as PrimaryKeyTrait>::ValueType>
        + Send
        + Sync
        + Clone
        + std::fmt::Debug;
    // =========================================================================
    // CORE DATABASE ACCESS
    // =========================================================================

    fn get_db(&self) -> &DatabaseConnection;

    // =========================================================================
    // COLUMN MAPPING (for dynamic queries)
    // =========================================================================

    fn id_column() -> Self::Column;
    fn email_column() -> Option<Self::Column> {
        None // Override if your entity has an email field
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

    /// Find entity by its primary key
    async fn find_by_id(&self, id: Self::Id) -> Result<Option<Self::Model>, sea_orm::DbErr> {
        Self::Entity::find_by_id(id).one(self.get_db()).await
    }

    /// Find all entities
    async fn find_all(&self) -> Result<Vec<Self::Model>, sea_orm::DbErr> {
        Self::Entity::find().all(self.get_db()).await
    }

    /// Find all entities with sorting
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

    /// Helper to get column from string name
    fn get_column(name: &str) -> Result<Self::Column, sea_orm::DbErr> {
        // This is a placeholder - in real implementation, you'd use reflection
        // or a mapping. For now, just return an error if you try to use it.
        Err(sea_orm::DbErr::Custom(format!(
            "Column '{}' not found",
            name
        )))
    }

    /// Find entities with pagination
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

    /// Find entities where a field equals a value
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

    /// Find single entity where a field equals a value
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

    /// Find entities where a field contains a substring (LIKE query)
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
    // EMAIL SPECIFIC METHODS (Optional)
    // =========================================================================

    /// Find by email (requires email_column to be implemented)
    async fn find_by_email(&self, email: &str) -> Result<Option<Self::Model>, sea_orm::DbErr> {
        if let Some(column) = Self::email_column() {
            Self::Entity::find()
                .filter(column.eq(email))
                .one(self.get_db())
                .await
        } else {
            Err(sea_orm::DbErr::Custom(
                "Email column not defined for this entity".to_string(),
            ))
        }
    }

    /// Check if email exists
    async fn exists_by_email(&self, email: &str) -> Result<bool, sea_orm::DbErr> {
        if let Some(column) = Self::email_column() {
            let count = Self::Entity::find()
                .filter(column.eq(email))
                .count(self.get_db())
                .await?;
            Ok(count > 0)
        } else {
            Err(sea_orm::DbErr::Custom(
                "Email column not defined for this entity".to_string(),
            ))
        }
    }

    // =========================================================================
    // SAVE / UPDATE OPERATIONS (JPA Style)
    // =========================================================================

    /// Save (insert) an entity
    async fn save(&self, model: Self::Model) -> Result<Self::Model, sea_orm::DbErr> {
        let active_model = <Self::ActiveModel as ConvertFromModel<Self::Model>>::from_model(model);
        let inserted_active = active_model.insert(self.get_db()).await?;
        inserted_active.try_into_model().map_err(|_| {
            sea_orm::DbErr::Custom("Failed to convert ActiveModel to Model".to_string())
        })
    }

    /// Save multiple entities in a batch
    async fn save_all(&self, models: Vec<Self::Model>) -> Result<Vec<Self::Model>, sea_orm::DbErr> {
        let mut results = Vec::new();
        for model in models {
            results.push(self.save(model).await?);
        }
        Ok(results)
    }

    /// Update an entity (assumes it already exists)
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

    /// Delete by primary key
    async fn delete_by_id(&self, id: Self::Id) -> Result<u64, sea_orm::DbErr> {
        let res = Self::Entity::delete_by_id(id).exec(self.get_db()).await?;
        Ok(res.rows_affected)
    }

    /// Delete an entity
    async fn delete(&self, model: &Self::Model) -> Result<u64, sea_orm::DbErr> {
        let active_model =
            <Self::ActiveModel as ConvertFromModel<Self::Model>>::from_model(model.clone());
        let res = active_model.delete(self.get_db()).await?;
        Ok(res.rows_affected)
    }

    /// Delete all entities
    async fn delete_all(&self) -> Result<u64, sea_orm::DbErr> {
        let res = Self::Entity::delete_many().exec(self.get_db()).await?;
        Ok(res.rows_affected)
    }

    /// Delete by condition
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

    /// Count all entities
    async fn count(&self) -> Result<u64, sea_orm::DbErr> {
        Self::Entity::find().count(self.get_db()).await
    }

    /// Count entities by field
    async fn count_by_field<F>(&self, column: Self::Column, value: F) -> Result<u64, sea_orm::DbErr>
    where
        F: Into<sea_orm::Value> + Send + Sync,
    {
        Self::Entity::find()
            .filter(column.eq(value))
            .count(self.get_db())
            .await
    }

    /// Check if entity exists by id
    async fn exists_by_id(&self, id: Self::Id) -> Result<bool, sea_orm::DbErr> {
        let count = Self::Entity::find_by_id(id).count(self.get_db()).await?;
        Ok(count > 0)
    }
}

// =============================================================================
// CONVERT FROM MODEL TRAIT
// =============================================================================

pub trait ConvertFromModel<M> {
    fn from_model(model: M) -> Self;
}