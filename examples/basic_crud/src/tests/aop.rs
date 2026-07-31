

#[cfg(test)]
mod tests {
    use gritshield::database::TxnRepository;
use sea_orm::ConnectionTrait;
    use sea_orm::PaginatorTrait;
    use crate::{
        models::user, repositories::user::UserRepository, services::transaction::{AUDIT_LOGGED_FAILURE, UserService},
    };
    use sea_orm::{Database, DatabaseConnection, EntityTrait, Schema};
    use std::sync::atomic::Ordering;

    async fn setup_test_db() -> DatabaseConnection {
        // Create an in-memory SQLite database
        let db = Database::connect("sqlite::memory:").await.unwrap();

        // Create the 'users' table schema
        let builder = db.get_database_backend();
        let schema = Schema::new(builder);
        let stmt = builder.build(&schema.create_table_from_entity(user::Entity));
        db.execute(stmt).await.unwrap();
        db
    }

    #[tokio::test]
    async fn test_transaction_rollback_and_interceptor() {
        let db = setup_test_db().await;
        let user_repo = UserRepository { db: db.clone() };
        let service = UserService {
            user_repo,
            db: db.clone(),
        };

        // Insert initial valid user
        service
            .create_user(1, "alice@gritshield.com".to_string())
            .await
            .expect("First insert should succeed");

        // Verify user 1 exists in the DB
        let count = user::Entity::find().count(&db).await.unwrap();
        assert_eq!(count, 1);

        // Attempt to insert a user with duplicate Primary Key (ID: 1)
        // This will trigger a DB execution failure inside `#[transactional]`
        let result = service
            .create_user(1, "bob@gritshield.com".to_string())
            .await;

        // VERIFICATION 1: The service method returned an Error
        assert!(
            result.is_err(),
            "Expected execution to fail on duplicate ID"
        );

        // VERIFICATION 2: Check database state -> Must still be 1 (Rollback succeeded!)
        let count_after_fail = user::Entity::find().count(&db).await.unwrap();
        assert_eq!(
            count_after_fail, 1,
            "Database state was not rolled back! Found extra records."
        );

        // VERIFICATION 3: Outer AuditLogger executed and saw the failure
        assert!(
            AUDIT_LOGGED_FAILURE.load(Ordering::SeqCst),
            "AuditLogger did not catch the rolled-back error"
        );
    }
}
