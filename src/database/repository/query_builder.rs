use sea_orm::{
    ColumnTrait, Condition, DatabaseConnection, DbErr, EntityTrait,
    QueryFilter, QueryOrder, Select, Value, QuerySelect,
};
use super::pagination::SortDirection;

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
