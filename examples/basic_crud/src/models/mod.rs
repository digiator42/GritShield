pub mod user;
pub mod post;
pub mod comment;
pub mod follower;

pub use user::Entity as UserEntity;
pub use post::Entity as PostEntity;
pub use comment::Entity as CommentEntity;
pub use follower::Entity as FollowersEntity;