use gritshield::deps::async_trait;
use gritshield::http::Response;
use gritshield::{
    controller, core::event_bus::GritEventHandler, event, routing::RequestContext, GritEvent,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(GritEvent, Clone, Serialize, Deserialize)]
pub struct UserRegistered {
    pub user_id: String,
    pub email: String,
}

pub struct WelcomeEmailHandler;

#[event]
impl WelcomeEmailHandler {
    pub async fn handle(&self, event: Arc<UserRegistered>) {
        println!("Sending email to: {}", event.email);
    }
}
pub struct EventController;

#[controller("/api/event")]
impl EventController {
    #[get("/register")]
    pub async fn register_user(ctx: RequestContext) -> Response {
        // 1. Execute primary user registration logic (e.g., save to DB)
        let user_id = "usr_99812".to_string();
        let email = "user@example.com".to_string();

        // 2. Instantiate the GritEvent struct
        let event = UserRegistered {
            user_id: user_id.clone(),
            email: email.clone(),
        };

        // 3. Publish to the event bus
        ctx.event_bus.publish(event);

        // 4. Return HTTP Response immediately without blocking on background handlers
        Response::ok(format!("User {} registered successfully!", user_id))
    }
}
