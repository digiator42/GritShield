use sea_orm::{
    ActiveModelBehavior, ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait,
    FromQueryResult, IntoActiveModel, LoaderTrait, ModelTrait, PaginatorTrait, PrimaryKeyTrait,
    QueryFilter, QueryOrder, QueryOrder as QueryOrderTrait, QueryResult, QuerySelect, Select,
    SelectTwoMany, TryIntoModel,
};
use sea_orm_migration::async_trait::async_trait;

use crate::deps::once_cell::sync::Lazy;
use crate::deps::sea_orm::DbErr;
use crate::protocol::response::Response;
use crate::routing::trie::RequestContext;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

// 🌟 Type alias for the type-erased admin dashboard request handlers
pub type AdminHandlerFn =
    Arc<dyn Fn(RequestContext) -> Pin<Box<dyn Future<Output = Response> + Send>> + Send + Sync>;

#[derive(Clone)]
pub struct ModelMetadata {
    pub table_name: &'static str,
    pub route_path: &'static str,
    pub searchable_columns: Vec<&'static str>,
    pub list_handler: AdminHandlerFn,
    pub search_handler: AdminHandlerFn,
    pub patch_handler: AdminHandlerFn,
}

// 🌟 The missing field parser trait required by macro expansion
pub trait AdminFieldParser {
    fn parse_field(s: &str) -> Result<Self, sea_orm::DbErr>
    where
        Self: Sized;
}

// Implement for standard strings
impl AdminFieldParser for String {
    fn parse_field(s: &str) -> Result<Self, sea_orm::DbErr> {
        Ok(s.to_string())
    }
}

// Implement for primitive scalar types
macro_rules! impl_primitive_parser {
    ($($t:ty),*) => {
        $(
            impl AdminFieldParser for $t {
                fn parse_field(s: &str) -> Result<Self, sea_orm::DbErr> {
                    s.parse::<Self>().map_err(|e| sea_orm::DbErr::Custom(format!("Parse error: {}", e)))
                }
            }
        )*
    };
}
impl_primitive_parser!(i8, i16, i32, i64, u8, u16, u32, u64, bool, f32, f64);

// 🌟 Existing Registry layout code below remains untouched
#[derive(Clone, Debug)]
pub struct GridColumn {
    pub name: &'static str,
    pub label: &'static str,
    pub is_editable: bool,
}

pub static ADMIN_REGISTRY: Lazy<Mutex<HashMap<&'static str, ModelMetadata>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

pub fn register_model(table: &'static str, meta: ModelMetadata) {
    if let Ok(mut registry) = ADMIN_REGISTRY.lock() {
        registry.insert(table, meta);
    }
}

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
    type Entity: EntityTrait<Model = Self::Model, Column = Self::Column>;
    type Model: ModelTrait + FromQueryResult + IntoActiveModel<Self::ActiveModel> + Send + Sync;
    type Column: ColumnTrait + Send + Sync;
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

    /// Performs updates directly onto records dynamically by string field names
    async fn update_column_value(
        &self,
        _id: Self::Id,
        _column_name: &str,
        _value: String,
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

    async fn find_by_id(&self, id: Self::Id) -> Result<Option<Self::Model>, sea_orm::DbErr> {
        Self::Entity::find_by_id(id).one(self.get_db()).await
    }

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
    // EMAIL SPECIFIC METHODS (Optional)
    // =========================================================================

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

    async fn delete_by_id(&self, id: Self::Id) -> Result<u64, sea_orm::DbErr> {
        let res = Self::Entity::delete_by_id(id).exec(self.get_db()).await?;
        Ok(res.rows_affected)
    }

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

    async fn count(&self) -> Result<u64, sea_orm::DbErr> {
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

    async fn exists_by_id(&self, id: Self::Id) -> Result<bool, sea_orm::DbErr> {
        let count = Self::Entity::find_by_id(id).count(self.get_db()).await?;
        Ok(count > 0)
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
