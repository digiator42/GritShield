use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Posts::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Posts::Id).integer().not_null().auto_increment().primary_key())
                    .col(ColumnDef::new(Posts::Title).string().not_null())
                    .col(ColumnDef::new(Posts::Text).text().not_null())
                    .col(ColumnDef::new(Posts::UserId).integer().not_null()) // The Foreign Key
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk-post-user_id")
                            .from(Posts::Table, Posts::UserId)
                            .to(Users::Table, Users::Id)
                            .on_delete(ForeignKeyAction::Cascade), // Delete posts if user is deleted
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(Posts::Table).to_owned()).await
    }
}

#[derive(DeriveIden)]
enum Posts { Table, Id, Title, Text, UserId }

#[derive(DeriveIden)]
enum Users { Table, Id } // Reference for the FK