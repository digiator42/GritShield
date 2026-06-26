use gritshield::prelude::*;

pub struct ApiController;

#[controller("/api")]
impl ApiController {
    #[get("/info")]
    pub async fn system_info(_ctx: RequestContext) -> Response {
        Response::ok("GritShield Engine Core Node Online.")
    }

    #[get("/health")]
    pub async fn health_check(_ctx: RequestContext) -> Response {
        Response::ok("OK")
    }
}
