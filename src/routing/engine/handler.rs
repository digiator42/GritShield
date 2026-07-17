use crate::http::response::Response;
use crate::security::errors::ShieldError;
use crate::security::xss::Sanitizer;
use super::context::RequestContext;
use futures::future::{BoxFuture, FutureExt};

pub type BoxedResponse = BoxFuture<'static, Response>;
pub type Handler = fn(RequestContext) -> BoxedResponse;
/// Short representation for handlers that can fail safely with an explicit framework error
pub type ShieldResult<T> = Result<T, ShieldError>;

pub trait IntoResponse {
    fn into_response(self) -> Response;
}

// A standard Response trivially turns into a Response
impl IntoResponse for Response {
    fn into_response(self) -> Response {
        self
    }
}

// Support raw static string slices: &'static str
impl IntoResponse for &'static str {
    fn into_response(self) -> Response {
        // Automatically wraps the text as an HTML response with a 200 OK status
        Response::new(200, Sanitizer::trust(self))
    }
}

// Support dynamic heap strings: String
impl IntoResponse for String {
    fn into_response(self) -> Response {
        Response::new(200, Sanitizer::trust(&self))
    }
}

// A ShieldResult turns into a Response by catching errors and invoking a fallback
impl IntoResponse for ShieldResult<Response> {
    fn into_response(self) -> Response {
        match self {
            Ok(res) => res,
            Err(err) => {
                println!(
                    "[SECURITY AUDIT] Handler caught an explicit framework error: {:?}",
                    err
                );

                // Determine status code and message based on the actual error type
                let (status, msg_string): (u16, String) = match err {
                    ShieldError::UnauthorizedAccess => {
                        (401, "<h1>401 Unauthorized</h1>".to_string())
                    }
                    ShieldError::Forbidden => (403, "<h1>403 Forbidden</h1>".to_string()),
                    ShieldError::NotFound => (404, "<h1>404 Not Found</h1>".to_string()),
                    ShieldError::BadRequest(err) => {
                        (400, format!("<h1>400 Bad Request</h1><br/>{}", err))
                    }
                    _ => (500, "<h1>500 Internal Security Error</h1>".to_string()),
                };

                // Pass the final String reference directly
                Response::new(status, Sanitizer::trust(&msg_string))
            }
        }
    }
}

pub trait IntoHandler: Send + Sync + 'static {
    fn call(&self, ctx: RequestContext) -> BoxedResponse;
}

// Add this blanket implementation to allow pre-boxed trait objects
impl IntoHandler for Box<dyn IntoHandler> {
    fn call(&self, ctx: RequestContext) -> BoxedResponse {
        // Delegate straight down to the inner trait object inside the box!
        self.as_ref().call(ctx)
    }
}

impl<F, Fut, R> IntoHandler for F
where
    F: Fn(RequestContext) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = R> + Send + 'static,
    R: IntoResponse + 'static,
{
    fn call(&self, ctx: RequestContext) -> BoxedResponse {
        let fut = (self)(ctx);

        async move {
            let res = fut.await;
            res.into_response()
        }
        .boxed()
    }
}

impl IntoHandler
    for std::sync::Arc<dyn Fn(RequestContext) -> BoxedResponse + Send + Sync + 'static>
{
    fn call(&self, ctx: RequestContext) -> BoxedResponse {
        (self)(ctx)
    }
}