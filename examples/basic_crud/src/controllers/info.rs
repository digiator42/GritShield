use gritshield::deps::sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use gritshield::{database::repository::GritRepository, prelude::*};
use serde_json::Value;

use crate::repositories::comment::CommentRepository;
use crate::repositories::post::PostRepository;
use crate::{
    models::{comment, post, user},
    repositories::user::UserRepository,
};

use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(GritSchema, Serialize, Deserialize)]
pub struct SwaggerTestData {
    pub email: String,
    pub name: String,
    pub age: Option<i32>,
}

pub struct ApiController;

#[controller("/api")]
impl ApiController {
    // ============================================================
    // ROUTE: /info
    // ============================================================
    #[get("/info")]
    pub async fn system_info(ctx: RequestContext) -> Response {
        let db = ctx.db.as_deref().unwrap().clone();

        let user_repo = UserRepository { db: db.clone() };

        let sea_user_with_posts = user::Entity::find()
            .filter(user::Column::Email.eq("user_1@example.com"))
            .find_with_related(post::Entity)
            .all(&db)
            .await
            .unwrap();

        let repo_user_with_posts = user_repo
            .find_by_email("user_1@example.com")
            .with_posts()
            .with_comments()
            .await
            .unwrap();

        let repo_user_query = user_repo
            .query()
            .where_gt(user::Column::Id, 3)
            .fetch()
            .await
            .unwrap();

        // =================== GritModel ====================
        let test_grit_model = user_repo
            .find_by_id_between(5, 6)
            .with_posts()
            .with_comments()
            .await
            .unwrap();

        let post_repo = PostRepository { db: db.clone() };

        let posts_with_comments = post_repo
            .find_by_id(2)
            .with_user()
            .with_comments()
            .await
            .unwrap();

        let posts_by_query = post_repo.search_admin_fields("03:15:07").await.unwrap();

        let user_tree = user_repo
            .find_by_email("user_1@example.com")
            .with_comments_nested(|query| query.with_user())
            .await
            .unwrap();

        let comment_repo = CommentRepository { db: db.clone() };

        let comments_with_all = comment_repo
            .find_by_id(2)
            .with_post()
            .with_user()
            .await
            .unwrap();

        Response::json(
            200,
            &json!({
                "sea_user_with_posts": sea_user_with_posts,
                "repo_user_with_posts": repo_user_with_posts,
                "repo_user_query": repo_user_query,
                "test_grit_model": test_grit_model,
                "posts_with_comments": posts_with_comments,
                "posts_by_query": posts_by_query,
                "user_tree": user_tree,
                "comments_with_all": comments_with_all,
            }),
        )
    }

    // ============================================================
    // ROUTE: /info/sea-orm
    // Test SeaORM native queries
    // ============================================================
    #[get("/info/sea-orm")]
    pub async fn test_sea_orm_queries(ctx: RequestContext) -> Response {
        let db = ctx.db.as_deref().unwrap().clone();

        // Test 1: Find user with posts (SeaORM native)
        let user_with_posts = match user::Entity::find()
            .filter(user::Column::Email.eq("user_1@example.com"))
            .find_with_related(post::Entity)
            .all(&db)
            .await
        {
            Ok(results) => results,
            Err(e) => return Response::bad_request(format!("SeaORM query failed: {}", e)),
        };

        // Test 2: Find all users with their posts
        let all_users_with_posts = match user::Entity::find()
            .find_with_related(post::Entity)
            .all(&db)
            .await
        {
            Ok(results) => results,
            Err(e) => return Response::bad_request(format!("SeaORM query failed: {}", e)),
        };

        // Test 3: Find posts with user (belongs_to)
        let posts_with_user = match post::Entity::find()
            .find_also_related(user::Entity)
            .all(&db)
            .await
        {
            Ok(results) => results,
            Err(e) => return Response::bad_request(format!("SeaORM query failed: {}", e)),
        };

        Response::json(
            200,
            &json!({
                "test": "SeaORM Native Queries",
                "user_with_posts": user_with_posts,
                "all_users_with_posts": all_users_with_posts,
                "posts_with_user": posts_with_user,
            }),
        )
    }

    // ============================================================
    // ROUTE: /info/grit-repo
    // Test GritRepository basic queries
    // ============================================================
    #[get("/info/grit-repo")]
    pub async fn test_grit_repository(ctx: RequestContext) -> Response {
        let db = ctx.db.as_deref().unwrap().clone();
        let user_repo = UserRepository { db: db.clone() };

        // Test 1: Find by email with relations
        let user_with_posts = match user_repo
            .find_by_email("user_1@example.com")
            .with_posts()
            .with_comments()
            .await
        {
            Ok(result) => result,
            Err(e) => return Response::bad_request(format!("Repository query failed: {}", e)),
        };

        // Test 2: Query builder with filter (no relations)
        let users_gt_3 = match user_repo
            .query()
            .where_gt(user::Column::Id, 3)
            .fetch()
            .await
        {
            Ok(results) => results,
            Err(e) => return Response::bad_request(format!("Query builder failed: {}", e)),
        };

        // Test 3: Find by ID range (GritModel)
        let users_range = match user_repo
            .find_by_id_between(5, 6)
            .with_posts()
            .with_comments()
            .await
        {
            Ok(results) => results,
            Err(e) => return Response::bad_request(format!("ID range query failed: {}", e)),
        };

        // Test 4: Search admin fields
        let users_search = match user_repo.search_admin_fields("user_1").await {
            Ok(results) => results,
            Err(e) => return Response::bad_request(format!("Search failed: {}", e)),
        };

        Response::json(
            200,
            &json!({
                "test": "GritRepository Basic Queries",
                "user_with_posts": user_with_posts,
                "users_gt_3": users_gt_3,
                "users_range": users_range,
                "users_search": users_search,
            }),
        )
    }

    // ============================================================
    // ROUTE: /info/posts
    // Test PostRepository specific queries
    // ============================================================
    #[get("/info/posts")]
    pub async fn test_post_repository(ctx: RequestContext) -> Response {
        let db = ctx.db.as_deref().unwrap().clone();
        let post_repo = PostRepository { db: db.clone() };

        // Test 1: Find post with user and comments
        let post_with_relations = match post_repo.find_by_id(2).with_user().with_comments().await {
            Ok(result) => result,
            Err(e) => return Response::bad_request(format!("Post query failed: {}", e)),
        };

        // Test 2: Search admin fields
        let posts_by_search = match post_repo.search_admin_fields("03:15").await {
            Ok(results) => results,
            Err(e) => return Response::bad_request(format!("Post search failed: {}", e)),
        };

        // Test 3: Query builder (no relations)
        let posts_by_user = match post_repo
            .query()
            .where_eq(post::Column::UserId, 1)
            .fetch()
            .await
        {
            Ok(results) => results,
            Err(e) => return Response::bad_request(format!("Post query failed: {}", e)),
        };

        Response::json(
            200,
            &json!({
                "test": "PostRepository Queries",
                "post_with_relations": post_with_relations,
                "posts_by_search": posts_by_search,
                "posts_by_user": posts_by_user,
            }),
        )
    }

    // ============================================================
    // ROUTE: /info/comments
    // Test CommentRepository queries
    // ============================================================
    #[get("/info/comments")]
    pub async fn test_comment_repository(ctx: RequestContext) -> Response {
        let db = ctx.db.as_deref().unwrap().clone();
        let comment_repo = CommentRepository { db: db.clone() };

        // Test 1: Find comment with post and user
        let comment_with_relations = match comment_repo.find_by_id(2).with_post().with_user().await
        {
            Ok(result) => result,
            Err(e) => return Response::bad_request(format!("Comment query failed: {}", e)),
        };

        // Test 2: Find all comments by user (QueryBuilder - no relations)
        let user_comments = match comment_repo
            .query()
            .where_eq(comment::Column::UserId, 1)
            .fetch()
            .await
        {
            Ok(results) => results,
            Err(e) => return Response::bad_request(format!("User comments query failed: {}", e)),
        };

        // Test 3: Search comments
        let comments_search = match comment_repo.search_admin_fields("test").await {
            Ok(results) => results,
            Err(e) => return Response::bad_request(format!("Comment search failed: {}", e)),
        };

        Response::json(
            200,
            &json!({
                "test": "CommentRepository Queries",
                "comment_with_relations": comment_with_relations,
                "user_comments": user_comments,
                "comments_search": comments_search,
            }),
        )
    }

    // ============================================================
    // ROUTE: /info/nested
    // Test nested relations (with_nested)
    // ============================================================
    #[get("/info/nested")]
    pub async fn test_nested_relations(ctx: RequestContext) -> Response {
        let db = ctx.db.as_deref().unwrap().clone();
        let user_repo = UserRepository { db: db.clone() };

        // Test nested: User -> Comments -> Post (self-referential through comments)
        let user_tree = match user_repo
            .find_by_email("user_1@example.com")
            .with_comments_nested(|query| query.with_post())
            .await
        {
            Ok(result) => result,
            Err(e) => return Response::bad_request(format!("Nested query failed: {}", e)),
        };

        // Test deep nesting: User -> Posts -> Comments -> User
        let deep_nested = match user_repo
            .find_by_email("user_1@example.com")
            .with_posts_nested(|query| query.with_comments_nested(|q| q.with_user()))
            .await
        {
            Ok(result) => result,
            Err(e) => return Response::bad_request(format!("Deep nested query failed: {}", e)),
        };

        Response::json(200, &deep_nested)
    }

    // ============================================================
    // ROUTE: /info/performance
    // Performance comparison between SeaORM and GritRepository
    // ============================================================
    #[get("/info/performance")]
    pub async fn test_performance(ctx: RequestContext) -> Response {
        use std::time::Instant;

        let db = ctx.db.as_deref().unwrap().clone();
        let user_repo = UserRepository { db: db.clone() };
        let post_repo = PostRepository { db: db.clone() };

        // ---- SeaORM ----
        let start = Instant::now();
        let sea_result = match user::Entity::find()
            .find_with_related(post::Entity)
            .all(&db)
            .await
        {
            Ok(r) => r,
            Err(e) => return Response::bad_request(format!("SeaORM failed: {}", e)),
        };
        let sea_duration = start.elapsed();

        // ---- Query Builder ----
        let start = Instant::now();
        let query_result = match post_repo
            .query()
            .where_gt(post::Column::Id, 1)
            .fetch()
            .await
        {
            Ok(r) => r,
            Err(e) => return Response::bad_request(format!("Query builder failed: {}", e)),
        };
        let query_duration = start.elapsed();

        Response::json(
            200,
            &json!({
                "test": "Performance Comparison",
                "seaorm": {
                    "duration_ms": sea_duration.as_millis(),
                    "count": sea_result.len(),
                },
                "query_builder": {
                    "duration_ms": query_duration.as_millis(),
                    "count": query_result.len(),
                },
            }),
        )
    }

    // ============================================================
    // ROUTE: /info/failures
    // Test error handling
    // ============================================================
    #[get("/info/failures")]
    pub async fn test_error_handling(ctx: RequestContext) -> Response {
        let db = ctx.db.as_deref().unwrap().clone();
        let user_repo = UserRepository { db: db.clone() };
        let post_repo = PostRepository { db: db.clone() };

        // Test 1: Non-existent record (find_by_id returns Option)
        let not_found = match user_repo.find_by_id(99999).await {
            Ok(result) => json!({ "found": true, "data": result }),
            Err(e) => json!({ "error": format!("{}", e) }),
        };

        // Test 2: Invalid email (find_by_email returns Option)
        let invalid_email = match user_repo.find_by_email("nonexistent@example.com").await {
            Ok(result) => json!({ "found": true, "data": result }),
            Err(e) => json!({ "error": format!("{}", e) }),
        };

        // Test 3: Empty query (fetch returns Vec)
        let empty_query = match post_repo
            .query()
            .where_eq(post::Column::Id, 99999)
            .fetch()
            .await
        {
            Ok(results) => json!({ "count": results.len(), "data": results }),
            Err(e) => json!({ "error": format!("{}", e) }),
        };

        Response::json(
            200,
            &json!({
                "test": "Error Handling",
                "not_found": not_found,
                "invalid_email": invalid_email,
                "empty_query": empty_query,
            }),
        )
    }

    // ============================================================
    // ROUTE: /info/all
    // Run all tests in one response (for quick verification)
    // ============================================================
    #[get("/info/all")]
    pub async fn test_all(ctx: RequestContext) -> Response {
        use std::time::Instant;
        let start = Instant::now();

        let db = ctx.db.as_deref().unwrap().clone();
        let user_repo = UserRepository { db: db.clone() };
        let post_repo = PostRepository { db: db.clone() };
        let comment_repo = CommentRepository { db: db.clone() };

        let results = json!({
            "basic_repo": {
                "user_with_posts": user_repo.find_by_email("user_1@example.com").with_posts().with_comments().await.ok(),
                "users_gt_3": user_repo.query().where_gt(user::Column::Id, 3).fetch().await.ok(),
                "users_range": user_repo.find_by_id_between(5, 6).with_posts().with_comments().await.ok(),
                "users_search": user_repo.search_admin_fields("user_1").await.ok(),
            },
            "posts": {
                "post_with_relations": post_repo.find_by_id(2).with_user().with_comments().await.ok(),
                "posts_by_search": post_repo.search_admin_fields("03:15:07").await.ok(),
                "posts_by_user": post_repo.query().where_eq(post::Column::UserId, 1).fetch().await.ok(),
            },
            "comments": {
                "comment_with_relations": comment_repo.find_by_id(2).with_post().with_user().await.ok(),
                "user_comments": comment_repo.query().where_eq(comment::Column::UserId, 1).fetch().await.ok(),
            },
            "nested": {
                "user_tree": user_repo.find_by_email("user_1@example.com").with_comments_nested(|q| q.with_user()).await.ok(),
            },
            "performance_ms": start.elapsed().as_millis(),
        });

        Response::json(200, &results)
    }

    // ============================================================
    // ROUTE: /info/social
    // Test social media relations with deep nesting
    // ============================================================
    #[get("/social")]
    pub async fn test_social_relations(ctx: RequestContext) -> Response {
        let db = match ctx.db.as_deref().clone() {
            Some(d) => d,
            None => return Response::bad_request("Database connection missing"),
        };

        let user_repo = UserRepository { db: db.clone() };

        // ---- Test 1: User with their social graph ----
        let user_with_social_list = match user_repo
            .find_by_id(5)
            .with_followerss()
            .with_followings()
            .with_posts()
            .await
        {
            Ok(result) => result,
            Err(e) => return Response::bad_request(format!("User social query failed: {}", e)),
        };

        // ---- Structural Serialization ----
        Response::json(200, &user_with_social_list)
    }

    // Use the body parameter in the route macro
    #[post("/swagger-body", body = SwaggerTestData)]
    pub async fn test_swagger_body(ctx: RequestContext) -> Response {
        // Parse the JSON body
        let data: Value = match ctx.json_body().await {
            Some(d) => d,
            None => Value::Null,
        };

        Response::ok(format!("Hello, {}!", data.get("name").unwrap()))
    }

    #[post("/swagger")]
    pub async fn test_swagger(ctx: RequestContext) -> Response {
        let db = ctx.db.as_deref().unwrap().clone();

        let data = ctx.form.fields;

        let user_repo = UserRepository { db: db.clone() };

        let sea_user_with_posts = user_repo
            .find_by_email(data["email"].as_str())
            .with_posts()
            .await
            .unwrap();

        Response::json(200, &sea_user_with_posts)
    }
}
