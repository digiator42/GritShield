mod index;
mod ws;

use gritshield::prelude::*;

#[tokio::main]
async fn main() {
    let mut router = Router::new();
    index::register_index_routes(&mut router);
    ignite("127.0.0.1", "8087", router).await;
}