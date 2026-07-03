use crate::deps::once_cell::sync::Lazy;
use crate::deps::sea_orm::sea_query::{Alias, Condition, Expr, JoinType, Query, SelectStatement};
use crate::deps::sea_orm::DbErr;
use crate::protocol::response::Response;
use crate::routing::trie::RequestContext;
use crate::security::xss::Sanitizer;
use sea_orm::{
    ActiveModelBehavior, ActiveModelTrait, ColumnTrait, DatabaseConnection, DbBackend, EntityTrait,
    FromQueryResult, IntoActiveModel, LoaderTrait, ModelTrait, PaginatorTrait, PrimaryKeyTrait,
    QueryFilter, QueryOrder, QueryOrder as QueryOrderTrait, QueryResult, QuerySelect, Select,
    SelectTwoMany, Statement, TryIntoModel, Value,
};
use sea_orm_migration::async_trait::async_trait;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

/// Simplified internal data structural representation of our custom search query
#[derive(Debug, Clone)]
pub struct CustomQuerySpec {
    pub base_table: String,
    pub select_columns: Vec<(Option<String>, String)>, // (Optional Table Prefix, Column Name)
    pub joins: Vec<JoinSpec>,
    pub r#where: Vec<WhereSpec>,
}

#[derive(Debug, Clone)]
pub struct JoinSpec {
    pub target_table: String,
    pub left_on: (String, String),  // (Table, Column)
    pub right_on: (String, String), // (Table, Column)
}

#[derive(Debug, Clone)]
pub struct WhereSpec {
    pub table: Option<String>,
    pub column: String,
    pub operator: String, // "=", "LIKE", ">"
    pub value: String,
}

/// Dynamic compiler translating structural specs into database-agnostic statements
pub struct JqlCompiler;

impl JqlCompiler {
    pub fn compile(spec: &CustomQuerySpec, backend: DbBackend) -> Statement {
        let mut select = Query::select();

        // 1. Process explicit select targets or fallback safely to wildcard definitions
        if spec.select_columns.is_empty() {
            select.column((Alias::new(&spec.base_table), Alias::new("*")));
        } else {
            for (table_opt, col) in &spec.select_columns {
                if let Some(tbl) = table_opt {
                    select.column((Alias::new(tbl), Alias::new(col)));
                } else {
                    select.column((Alias::new(&spec.base_table), Alias::new(col)));
                }
            }
        }

        // 2. Define root origin table target matrix
        select.from(Alias::new(&spec.base_table));

        // 3. Append relational joins dynamically
        for join in &spec.joins {
            let left_expr = Expr::col((Alias::new(&join.left_on.0), Alias::new(&join.left_on.1)));
            let right_expr =
                Expr::col((Alias::new(&join.right_on.0), Alias::new(&join.right_on.1)));

            // Cleanly link the two column expression definitions together
            select.join(
                JoinType::InnerJoin,
                Alias::new(&join.target_table),
                left_expr.eq(right_expr),
            );
        }

        // 4. Inject runtime query condition filters safely
        let mut conditions = Condition::all();
        for cond in &spec.r#where {
            let col_ref = if let Some(tbl) = &cond.table {
                Expr::col((Alias::new(tbl), Alias::new(&cond.column)))
            } else {
                Expr::col((Alias::new(&spec.base_table), Alias::new(&cond.column)))
            };

            // Fix: Wrap raw strings inside Expr::val to yield structural parameters
            let clause = match cond.operator.as_str() {
                "=" => col_ref.eq(Expr::val(cond.value.clone())),
                "LIKE" => col_ref.like(format!("%{}%", cond.value)),
                ">" => col_ref.gt(Expr::val(cond.value.clone())),
                "<" => col_ref.lt(Expr::val(cond.value.clone())),
                _ => col_ref.eq(Expr::val(cond.value.clone())),
            };
            conditions = conditions.add(clause);
        }

        select.cond_where(conditions);

        // 5. Generate target-compiled SQL variant safely
        backend.build(&select)
    }
}

impl CustomQuerySpec {
    pub fn parse_from_str(input: &str) -> Result<Self, String> {
        let input = Sanitizer::url_decode(input).replace(";", "");
        let normalized = input.replace(",", " ").to_lowercase();
        let tokens: Vec<&str> = normalized.split_whitespace().collect();

        println!("====>> {:?}\n{:?}", input, tokens);

        // 1. Detect core indexing components
        let select_idx = tokens.iter().position(|&t| t == "select");
        let from_idx = tokens.iter().position(|&t| t == "from");
        let join_idx = tokens.iter().position(|&t| t == "join");
        let where_idx = tokens.iter().position(|&t| t == "where");

        if select_idx.is_none() || from_idx.is_none() {
            return Err(
                "Invalid structural syntax: Queries must include SELECT and FROM clauses."
                    .to_string(),
            );
        }

        let from_table = tokens[from_idx.unwrap() + 1].to_string();
        let mut select_columns = Vec::new();

        // 2. Parse target columns
        for i in (select_idx.unwrap() + 1)..from_idx.unwrap() {
            let part = tokens[i];
            if part.contains('.') {
                let chunks: Vec<&str> = part.split('.').collect();
                select_columns.push((Some(chunks[0].to_string()), chunks[1].to_string()));
            } else {
                select_columns.push((None, part.to_string()));
            }
        }

        // 3. Extract dynamic joins if present
        let mut joins = Vec::new();
        if let Some(j_idx) = join_idx {
            let on_idx = tokens.iter().position(|&t| t == "on");
            if let Some(o_idx) = on_idx {
                let target_table = tokens[j_idx + 1].to_string();
                let left_side = tokens[o_idx + 1].split('.').collect::<Vec<&str>>();
                let right_side = tokens[o_idx + 3].split('.').collect::<Vec<&str>>();

                joins.push(JoinSpec {
                    target_table,
                    left_on: (left_side[0].to_string(), left_side[1].to_string()),
                    right_on: (right_side[0].to_string(), right_side[1].to_string()),
                });
            }
        }

        // 4. Extract where condition targets
        let mut conditions = Vec::new();
        if let Some(w_idx) = where_idx {
            let col_part = tokens[w_idx + 1];
            let operator = tokens[w_idx + 2].to_string();
            let value = tokens[w_idx + 3].replace("'", "").to_string();

            if col_part.contains('.') {
                let chunks: Vec<&str> = col_part.split('.').collect();
                conditions.push(WhereSpec {
                    table: Some(chunks[0].to_string()),
                    column: chunks[1].to_string(),
                    operator,
                    value,
                });
            } else {
                conditions.push(WhereSpec {
                    table: None,
                    column: col_part.to_string(),
                    operator,
                    value,
                });
            }
        }

        Ok(CustomQuerySpec {
            base_table: from_table,
            select_columns,
            joins,
            r#where: conditions,
        })
    }
}

// Type alias for the type-erased admin dashboard request handlers
pub type AdminHandlerFn =
    Arc<dyn Fn(RequestContext) -> Pin<Box<dyn Future<Output = Response> + Send>> + Send + Sync>;

#[derive(Clone)]
pub struct ModelMetadata {
    pub table_name: &'static str,
    pub route_path: &'static str,
    pub searchable_columns: Vec<&'static str>,
    pub list_handler: AdminHandlerFn,
    pub search_handler: AdminHandlerFn,
    pub delete_handler: AdminHandlerFn,
    pub patch_handler: AdminHandlerFn,
    pub advanced_search_handler: AdminHandlerFn,
    pub detail_handler: AdminHandlerFn,
    pub bulk_delete_handler: AdminHandlerFn,
    pub export_handler: AdminHandlerFn,
}

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

// Existing Registry layout code below remains untouched
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

#[derive(Clone)]
struct OrderClause<C>
where
    C: ColumnTrait,
{
    column: C,
    direction: SortDirection,
}

pub struct QueryBuilder<E>
where
    E: EntityTrait,
{
    db: DatabaseConnection,

    filters: sea_orm::Condition,

    order_by: Vec<OrderClause<E::Column>>,

    limit: Option<u64>,

    offset: Option<u64>,
}

impl<E> QueryBuilder<E>
where
    E: EntityTrait,
    <E as EntityTrait>::Model: Send + Sync,
{
    pub fn new(db: DatabaseConnection) -> Self {
        Self {
            db,
            filters: Condition::all(),
            order_by: Vec::new(),
            limit: None,
            offset: None,
        }
    }

    fn build_select(&self) -> Select<E> {
        let mut query = E::find().filter(self.filters.clone());

        for order in &self.order_by {
            match order.direction {
                SortDirection::Asc => {
                    query = query.order_by_asc(order.column.clone());
                }
                SortDirection::Desc => {
                    query = query.order_by_desc(order.column.clone());
                }
            }
        }

        if let Some(limit) = self.limit {
            query = query.limit(limit);
        }

        if let Some(offset) = self.offset {
            query = query.offset(offset);
        }

        query
    }

    pub async fn fetch(self) -> Result<Vec<E::Model>, DbErr> {
        self.build_select().all(&self.db).await
    }

    pub async fn fetch_one(self) -> Result<E::Model, DbErr> {
        self.build_select()
            .one(&self.db)
            .await?
            .ok_or(DbErr::RecordNotFound("Expected one record".into()))
    }

    pub async fn count(self) -> Result<u64, DbErr> {
        let query = self.build_select();

        sea_orm::PaginatorTrait::count(query, &self.db).await
    }

    pub async fn exists(self) -> Result<bool, DbErr> {
        Ok(self.build_select().one(&self.db).await?.is_some())
    }

    pub async fn delete(self) -> Result<sea_orm::DeleteResult, DbErr> {
        E::delete_many().filter(self.filters).exec(&self.db).await
    }

    pub fn where_eq<C, V>(mut self, column: C, value: V) -> Self
    where
        C: ColumnTrait,
        V: Into<Value>,
    {
        self.filters = self.filters.add(column.eq(value));

        self
    }

    // =================================================================

    pub async fn fetch_optional(self) -> Result<Option<E::Model>, DbErr> {
        self.build_select().one(&self.db).await
    }

    pub async fn first(self) -> Result<Option<E::Model>, DbErr> {
        self.build_select().one(&self.db).await
    }

    pub fn limit(mut self, limit: u64) -> Self {
        self.limit = Some(limit);
        self
    }

    pub fn offset(mut self, offset: u64) -> Self {
        self.offset = Some(offset);
        self
    }

    pub fn paginate(mut self, page: u64, size: u64) -> Self {
        self.offset = Some(page * size);
        self.limit = Some(size);
        self
    }

    // ============ Equality ==============

    pub fn where_ne<C, V>(mut self, column: C, value: V) -> Self
    where
        C: ColumnTrait,
        V: Into<Value>,
    {
        self.filters = self.filters.add(column.ne(value));
        self
    }

    // ========= Comparison ============

    pub fn where_gt<C, V>(mut self, column: C, value: V) -> Self
    where
        C: ColumnTrait,
        V: Into<Value>,
    {
        self.filters = self.filters.add(column.gt(value));
        self
    }

    pub fn where_ge<C, V>(mut self, column: C, value: V) -> Self
    where
        C: ColumnTrait,
        V: Into<Value>,
    {
        self.filters = self.filters.add(column.gte(value));
        self
    }

    pub fn where_lt<C, V>(mut self, column: C, value: V) -> Self
    where
        C: ColumnTrait,
        V: Into<Value>,
    {
        self.filters = self.filters.add(column.lt(value));
        self
    }

    pub fn where_le<C, V>(mut self, column: C, value: V) -> Self
    where
        C: ColumnTrait,
        V: Into<Value>,
    {
        self.filters = self.filters.add(column.lte(value));
        self
    }

    // ========== Collection ===========

    pub fn where_in<C, I, V>(mut self, column: C, values: I) -> Self
    where
        C: ColumnTrait,
        I: IntoIterator<Item = V>,
        V: Into<Value>,
    {
        self.filters = self.filters.add(column.is_in(values));
        self
    }

    pub fn where_not_in<C, I, V>(mut self, column: C, values: I) -> Self
    where
        C: ColumnTrait,
        I: IntoIterator<Item = V>,
        V: Into<Value>,
    {
        self.filters = self.filters.add(column.is_not_in(values));
        self
    }

    // ========= Range ==========

    pub fn where_between<C, V>(mut self, column: C, low: V, high: V) -> Self
    where
        C: ColumnTrait,
        V: Into<Value>,
    {
        self.filters = self.filters.add(column.between(low, high));
        self
    }

    pub fn where_null<C>(mut self, column: C) -> Self
    where
        C: ColumnTrait,
    {
        self.filters = self.filters.add(column.is_null());
        self
    }

    pub fn where_not_null<C>(mut self, column: C) -> Self
    where
        C: ColumnTrait,
    {
        self.filters = self.filters.add(column.is_not_null());
        self
    }

    pub fn where_like<C>(mut self, column: C, pattern: impl Into<String>) -> Self
    where
        C: ColumnTrait,
    {
        self.filters = self.filters.add(column.like(pattern.into()));
        self
    }

    pub fn where_contains<C>(mut self, column: C, value: impl AsRef<str>) -> Self
    where
        C: ColumnTrait,
    {
        self.filters = self
            .filters
            .add(column.like(format!("%{}%", value.as_ref())));
        self
    }

    pub fn where_starts_with<C>(mut self, column: C, value: impl AsRef<str>) -> Self
    where
        C: ColumnTrait,
    {
        self.filters = self
            .filters
            .add(column.like(format!("{}%", value.as_ref())));
        self
    }

    pub fn order_asc(mut self, column: E::Column) -> Self {
        self.order_by.push(OrderClause {
            column,
            direction: SortDirection::Asc,
        });
        self
    }

    pub fn order_desc(mut self, column: E::Column) -> Self {
        self.order_by.push(OrderClause {
            column,
            direction: SortDirection::Desc,
        });
        self
    }
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

    /// Insert an audit log entry. Default implementation does nothing.
    async fn audit_log(
        &self,
        table_name: &str,
        record_id: &str,
        action: &str,
        old_values: Option<serde_json::Value>,
        new_values: Option<serde_json::Value>,
        user_id: Option<&str>,
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
        user_id: Option<&str>,
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
