use crate::routing::engine::RequestContext;
use crate::middleware::{Middleware, MiddlewareResult};
use colored::*;
use chrono::Local;

pub struct LoggerMiddleware;

impl Middleware for LoggerMiddleware {
    fn execute(&self, ctx: &mut RequestContext) -> MiddlewareResult {
        let now = Local::now();
        let timestamp = now.format("%Y-%m-%d %H:%M:%S").to_string();

        println!(
            "[{}] {} request to {}",
            timestamp.green(),
            format!("{:?}", ctx.req.method).blue(),
            ctx.req.path.yellow()
        );

        MiddlewareResult::Next(None)
    }
}