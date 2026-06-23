use sea_orm::{
    ActiveModelBehavior, ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait,
    FromQueryResult, IntoActiveModel, ModelTrait, PrimaryKeyTrait, QueryFilter, TryIntoModel,
};
use sea_orm_migration::async_trait::async_trait;

#[async_trait]
pub trait GritRepository {
    // Core Sea-ORM Entity Type Mapping Relationships
    type Entity: EntityTrait<Model = Self::Model, Column = Self::Column>;

    // Added FromQueryResult bound constraint here
    type Model: ModelTrait + FromQueryResult + IntoActiveModel<Self::ActiveModel> + Send + Sync;

    // Explicitly declared the associated Column type on the trait
    type Column: ColumnTrait + Send + Sync;

    // Added ActiveModelBehavior to the type bounds here
    type ActiveModel: ActiveModelTrait<Entity = Self::Entity>
        + ActiveModelBehavior
        + TryIntoModel<Self::Model>
        + ConvertFromModel<Self::Model>
        + Send
        + Sync;

    // Get the database connection pool
    fn get_db(&self) -> &DatabaseConnection;

    // Provide the column mapping requirement for inheritance
    fn email_column() -> Self::Column;

    // findByEmail(String email)
    async fn find_by_email(&self, email: &str) -> Result<Option<Self::Model>, sea_orm::DbErr> {
        Self::Entity::find()
            .filter(Self::email_column().eq(email))
            .one(self.get_db())
            .await
    }

    // findById(ID id)
    async fn find_by_id(
        &self,
        id: <<Self::Entity as EntityTrait>::PrimaryKey as PrimaryKeyTrait>::ValueType,
    ) -> Result<Option<Self::Model>, sea_orm::DbErr>
    where
        <<Self::Entity as EntityTrait>::PrimaryKey as PrimaryKeyTrait>::ValueType: Send + Sync,
    {
        Self::Entity::find_by_id(id).one(self.get_db()).await
    }

    // findAll()
    async fn find_all(&self) -> Result<Vec<Self::Model>, sea_orm::DbErr> {
        Self::Entity::find().all(self.get_db()).await
    }

    // Spring JPA: save(S entity)
    async fn save(&self, model: Self::Model) -> Result<Self::Model, sea_orm::DbErr> {
        let active_model = <Self::ActiveModel as ConvertFromModel<Self::Model>>::from_model(model);
        let inserted_active = active_model.insert(self.get_db()).await?;

        inserted_active.try_into_model().map_err(|_| {
            sea_orm::DbErr::Custom(
                "Failed to convert ActiveModel back into Model target type".to_owned(),
            )
        })
    }

    // deleteById(ID id)
    async fn delete_by_id(
        &self,
        id: <<Self::Entity as EntityTrait>::PrimaryKey as PrimaryKeyTrait>::ValueType,
    ) -> Result<u64, sea_orm::DbErr>
    where
        <<Self::Entity as EntityTrait>::PrimaryKey as PrimaryKeyTrait>::ValueType: Send + Sync,
    {
        let res = Self::Entity::delete_by_id(id).exec(self.get_db()).await?;
        Ok(res.rows_affected)
    }
}

pub trait ConvertFromModel<M> {
    fn from_model(model: M) -> Self;
}
