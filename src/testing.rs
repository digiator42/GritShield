//! Test doubles for WebSocket routes.
//!
//! Writing a `WebSocketHandler` test by hand means hand-rolling a listener, a
//! `Router`, an accept loop, and a client handshake every single time — and the
//! parts that usually break (the handshake bytes, the upgrade boundary, the
//! negotiated subprotocol) are exactly the parts that are hardest to inspect
//! once the client library owns them.
//!
//! [`WsTestServer`] binds an ephemeral port, runs the framework's real
//! `handle_connection` against a real [`Router`], and hands back either an
//! upgraded connection or the literal bytes of the HTTP response it refused
//! with. Nothing here stubs out the framework, so a test using it exercises the
//! same code path a production server does.
//!
//! ```no_run
//! use gritshield::testing::WsTestServer;
//!
//! # async fn example() {
//! let server = WsTestServer::start().await;
//! gritshield::routing::websocket::register_ws_route("/ws/echo", |stream, ctx| {
//!     let (conn, _sink) = gritshield::routing::websocket::WsConnection::new(MyEcho, ctx);
//!     Box::pin(conn.run(stream))
//! });
//!
//! let mut client = server.connect("/ws/echo").await;
//! client.send_text("hello").await.unwrap();
//! assert_eq!(client.expect_text().await, "hello");
//! # }
//! # struct MyEcho;
//! # impl gritshield::routing::websocket::WebSocketHandler for MyEcho {
//! #     type Message = String;
//! #     fn on_message(
//! #         &self,
//! #         msg: String,
//! #         _ctx: &gritshield::routing::engine::RequestContext,
//! #         ws: gritshield::routing::websocket::WsSink,
//! #     ) -> gritshield::routing::websocket::BoxedWsFuture {
//! #         Box::pin(async move { let _ = ws.send_text(msg); })
//! #     }
//! # }
//! ```

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use crate::deps::futures_util::{SinkExt, StreamExt};
use crate::deps::tokio_tungstenite::tungstenite::protocol::Role;
use crate::deps::tokio_tungstenite::tungstenite::Message;
use crate::deps::tokio_tungstenite::WebSocketStream;
use crate::http::handle_connection;
use crate::routing::engine::Router;

/// A fixed, well-formed `Sec-WebSocket-Key`.
///
/// RFC 6455 wants 16 random bytes, base64'd. A constant is fine for a test
/// harness: the server only hashes what it is given, and a constant keeps the
/// expected accept value reproducible.
pub const TEST_WS_KEY: &str = "dGhlIHNhbXBsZSBub25jZQ==";

/// The only WebSocket version this framework speaks.
pub const TEST_WS_VERSION: &str = "13";

/// How long a harness read waits before giving up.
///
/// Generous enough for a loaded CI box, short enough that a wedged handler fails
/// the test instead of the suite.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);

/// An ephemeral HTTP server running the framework's real connection loop.
///
/// Dropping the server does not close already-accepted sockets; each accepted
/// connection is a detached task, exactly as in production.
pub struct WsTestServer {
    addr: SocketAddr,
    router: Arc<Router>,
}

impl WsTestServer {
    /// Binds an ephemeral port on loopback and starts serving in the background.
    pub async fn start() -> Self {
        Self::start_with_router(Router::new()).await
    }

    /// Like [`Self::start`], but serves `router` as given, so tests can
    /// register routes, middleware and after-hooks before the first request.
    pub async fn start_with_router(router: Router) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("failed to bind an ephemeral test port");
        let addr = listener
            .local_addr()
            .expect("bound listener has no local address");
        let router = Arc::new(router);

        let accept_router = router.clone();
        tokio::spawn(async move {
            while let Ok((stream, peer)) = listener.accept().await {
                let router = accept_router.clone();
                tokio::spawn(async move {
                    handle_connection(stream, peer, router).await;
                });
            }
        });

        Self { addr, router }
    }

    /// The address the server is listening on.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// The `ws://` URL for `path`, e.g. `ws://127.0.0.1:34567/ws/echo`.
    pub fn url(&self, path: &str) -> String {
        format!("ws://{}{}", self.addr, path)
    }

    /// The router the server is serving, for tests that assert on metrics.
    pub fn router(&self) -> &Arc<Router> {
        &self.router
    }

    /// Performs a full handshake and returns an upgraded connection.
    ///
    /// # Panics
    ///
    /// Panics if the server refuses the upgrade. In a test that called `connect`,
    /// a refusal means routing or the handshake is broken, and the caller should
    /// not have to dig through a status code to find out. Use [`Self::try_connect`]
    /// when the refusal *is* the thing under test.
    pub async fn connect(&self, path: &str) -> WsTestClient {
        self.try_connect(path, &[])
            .await
            .unwrap_or_else(|failure| panic!("{}", failure))
    }

    /// Like [`Self::connect`], but offers `subprotocols` during the handshake.
    ///
    /// # Panics
    ///
    /// Panics if the server refuses the upgrade.
    pub async fn connect_with_subprotocols(
        &self,
        path: &str,
        subprotocols: &[&str],
    ) -> WsTestClient {
        self.try_connect(path, subprotocols)
            .await
            .unwrap_or_else(|failure| panic!("{}", failure))
    }

    /// Handshakes against `path`, surfacing a refusal instead of panicking.
    pub async fn try_connect(
        &self,
        path: &str,
        subprotocols: &[&str],
    ) -> Result<WsTestClient, WsHandshakeFailure> {
        self.handshake(path, subprotocols, TEST_WS_VERSION, true).await
    }

    /// Handshakes against a WebSocket route with a deliberately wrong protocol
    /// version, which RFC 6455 §4.2.1 requires be answered with `426`.
    pub async fn try_connect_with_version(
        &self,
        path: &str,
        version: &str,
    ) -> Result<WsTestClient, WsHandshakeFailure> {
        self.handshake(path, &[], version, true).await
    }

    /// Handshakes without sending `Sec-WebSocket-Key` at all.
    pub async fn try_connect_without_key(&self, path: &str) -> Result<WsTestClient, WsHandshakeFailure> {
        self.handshake(path, &[], TEST_WS_VERSION, false).await
    }

    /// Handshakes with an `Upgrade` header the server does not understand.
    pub async fn try_connect_without_upgrade(&self, path: &str) -> Result<WsTestClient, WsHandshakeFailure> {
        let mut stream = self.tcp_connect().await?;

        let request = format!(
            "GET {} HTTP/1.1\r\nHost: {}\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {}\r\nSec-WebSocket-Version: {}\r\n\r\n",
            path, self.addr, TEST_WS_KEY, TEST_WS_VERSION
        );
        write_request(&mut stream, &request).await?;

        let response = read_http_response(&mut stream).await?;
        Err(WsHandshakeFailure::from_response(response))
    }

    /// Sends a plain (non-upgrade) HTTP request and returns the raw response.
    ///
    /// This is a general-purpose HTTP harness, not WebSocket-specific: any
    /// method, any extra request headers. `Connection: close` keeps the read
    /// path trivial and matches how one-shot HTTP tests behave.
    pub async fn request(
        &self,
        method: &str,
        path: &str,
        headers: &[(&str, &str)],
    ) -> WsHttpResponse {
        let mut stream = self.tcp_connect().await.expect("server is listening");

        let mut request = format!(
            "{} {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n",
            method, path, self.addr
        );
        for (name, value) in headers {
            request.push_str(&format!("{}: {}\r\n", name, value));
        }
        request.push_str("\r\n");

        write_request(&mut stream, &request)
            .await
            .expect("writing a plain request");

        read_http_response(&mut stream)
            .await
            .expect("reading a plain request response")
    }

    /// Sends a plain (non-upgrade) `GET` to a path and returns the raw response.
    ///
    /// Used to assert that a WebSocket route answers a browser navigation with
    /// `426 Upgrade Required` rather than pretending the route does not exist.
    pub async fn get(&self, path: &str) -> WsHttpResponse {
        self.request("GET", path, &[]).await
    }

    async fn tcp_connect(&self) -> Result<TcpStream, WsHandshakeFailure> {
        TcpStream::connect(self.addr)
            .await
            .map_err(WsHandshakeFailure::io)
    }

    /// Writes the handshake request by hand so the test controls every header,
    /// then either upgrades the socket or reports the refusal.
    async fn handshake(
        &self,
        path: &str,
        subprotocols: &[&str],
        version: &str,
        send_key: bool,
    ) -> Result<WsTestClient, WsHandshakeFailure> {
        let mut stream = self.tcp_connect().await?;

        let mut request = format!(
            "GET {} HTTP/1.1\r\nHost: {}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n",
            path, self.addr
        );
        if send_key {
            request.push_str(&format!("Sec-WebSocket-Key: {}\r\n", TEST_WS_KEY));
        }
        request.push_str(&format!("Sec-WebSocket-Version: {}\r\n", version));
        if !subprotocols.is_empty() {
            request.push_str(&format!(
                "Sec-WebSocket-Protocol: {}\r\n",
                subprotocols.join(", ")
            ));
        }
        request.push_str("\r\n");

        write_request(&mut stream, &request).await?;

        let response = read_http_response(&mut stream).await?;

        if response.status != 101 {
            return Err(WsHandshakeFailure::from_response(response));
        }

        let subprotocol = response.header("sec-websocket-protocol");

        // The head was read one byte at a time, so nothing belonging to the
        // first WebSocket frame is still sitting in our buffer.
        let ws_stream = WebSocketStream::from_raw_socket(stream, Role::Client, None).await;

        Ok(WsTestClient {
            stream: ws_stream,
            subprotocol,
            addr: self.addr,
        })
    }
}

/// A client-side WebSocket connected to a [`WsTestServer`].
pub struct WsTestClient {
    stream: WebSocketStream<TcpStream>,
    subprotocol: Option<String>,
    addr: SocketAddr,
}

impl WsTestClient {
    /// The subprotocol the server selected during the handshake, if any.
    pub fn subprotocol(&self) -> Option<&str> {
        self.subprotocol.as_deref()
    }

    /// Sends a text frame.
    pub async fn send_text(&mut self, text: impl Into<String>) -> Result<(), WsTestError> {
        self.stream
            .send(Message::Text(text.into()))
            .await
            .map_err(WsTestError::Transport)
    }

    /// Sends a binary frame.
    pub async fn send_binary(&mut self, bytes: impl Into<Vec<u8>>) -> Result<(), WsTestError> {
        self.stream
            .send(Message::Binary(bytes.into()))
            .await
            .map_err(WsTestError::Transport)
    }

    /// Returns the next frame, or `None` if the server closed first.
    pub async fn next_frame(&mut self) -> Result<Option<Message>, WsTestError> {
        match tokio::time::timeout(DEFAULT_TIMEOUT, self.stream.next()).await {
            Err(_) => Err(WsTestError::Timeout),
            Ok(None) => Ok(None),
            Ok(Some(frame)) => frame.map(Some).map_err(WsTestError::Transport),
        }
    }

    /// Returns the next frame decoded as UTF-8 text.
    pub async fn next_text(&mut self) -> Result<String, WsTestError> {
        match self.next_frame().await? {
            Some(Message::Text(text)) => Ok(text),
            Some(other) => Err(WsTestError::UnexpectedFrame(format!("{:?}", other))),
            None => Err(WsTestError::Closed),
        }
    }

    /// Like [`Self::next_text`], but fails the calling test on anything else.
    ///
    /// # Panics
    ///
    /// Panics when the server is silent, sends a non-text frame, or hangs up.
    pub async fn expect_text(&mut self) -> String {
        self.next_text()
            .await
            .unwrap_or_else(|err| panic!("{}", err))
    }

    /// Reads text frames until one satisfies `predicate`, or fails the test.
    ///
    /// # Panics
    ///
    /// Panics if the connection closes, goes quiet, or never matches.
    pub async fn expect_text_matching<F>(&mut self, predicate: F) -> String
    where
        F: Fn(&str) -> bool,
    {
        for _ in 0..64 {
            let text = self.expect_text().await;
            if predicate(&text) {
                return text;
            }
        }
        panic!("no matching frame arrived within 64 reads");
    }

    /// Sends a close frame and waits for the server to hang up.
    pub async fn close(&mut self) -> Result<(), WsTestError> {
        self.stream
            .close(None)
            .await
            .map_err(WsTestError::Transport)
    }

    /// Drops the socket without a close handshake, simulating an abrupt peer.
    pub fn abort(&mut self) {
        let _ = self.stream.get_mut().shutdown();
    }
}

impl std::fmt::Debug for WsTestClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WsTestClient")
            .field("addr", &self.addr)
            .field("subprotocol", &self.subprotocol)
            .finish()
    }
}

/// A raw HTTP response, as read off the wire by the harness.
#[derive(Debug, Clone)]
pub struct WsHttpResponse {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: String,
}

impl WsHttpResponse {
    /// Case-insensitive header lookup.
    pub fn header(&self, name: &str) -> Option<String> {
        self.headers.get(&name.to_ascii_lowercase()).cloned()
    }
}

/// Why an upgrade did not happen.
#[derive(Debug)]
pub enum WsHandshakeFailure {
    /// The server answered with something other than `101`.
    Rejected(WsHttpResponse),
    /// The socket failed before a response was read.
    Io(std::io::Error),
    /// The response was not parseable as HTTP.
    Malformed(String),
}

impl WsHandshakeFailure {
    /// The status the server answered with, when there was one.
    pub fn status(&self) -> Option<u16> {
        match self {
            WsHandshakeFailure::Rejected(resp) => Some(resp.status),
            _ => None,
        }
    }

    /// The response body, when there was one.
    pub fn body(&self) -> Option<&str> {
        match self {
            WsHandshakeFailure::Rejected(resp) => Some(&resp.body),
            _ => None,
        }
    }

    /// The value of a response header, when there was one.
    pub fn header(&self, name: &str) -> Option<String> {
        match self {
            WsHandshakeFailure::Rejected(resp) => resp.header(name),
            _ => None,
        }
    }

    fn io(err: std::io::Error) -> Self {
        WsHandshakeFailure::Io(err)
    }

    fn from_response(response: WsHttpResponse) -> Self {
        WsHandshakeFailure::Rejected(response)
    }
}

impl std::fmt::Display for WsHandshakeFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WsHandshakeFailure::Rejected(resp) => write!(
                f,
                "server refused the upgrade with status {} and body {:?}",
                resp.status, resp.body
            ),
            WsHandshakeFailure::Io(err) => write!(f, "handshake socket failed: {}", err),
            WsHandshakeFailure::Malformed(why) => {
                write!(f, "handshake response was malformed: {}", why)
            }
        }
    }
}

impl std::error::Error for WsHandshakeFailure {}

/// Why a harness read or write did not produce what the test expected.
#[derive(Debug)]
pub enum WsTestError {
    /// The socket failed mid-conversation.
    Transport(tokio_tungstenite::tungstenite::Error),
    /// The server said nothing within the harness timeout.
    Timeout,
    /// The server hung up.
    Closed,
    /// A frame arrived, but not the kind the test asked for.
    UnexpectedFrame(String),
}

impl std::fmt::Display for WsTestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WsTestError::Transport(err) => write!(f, "websocket transport failed: {}", err),
            WsTestError::Timeout => write!(
                f,
                "no frame arrived within {:?} -- the server appears to be wedged",
                DEFAULT_TIMEOUT
            ),
            WsTestError::Closed => write!(f, "server closed the connection"),
            WsTestError::UnexpectedFrame(frame) => {
                write!(f, "expected a text frame, got {}", frame)
            }
        }
    }
}

impl std::error::Error for WsTestError {}

async fn write_request(
    stream: &mut TcpStream,
    request: &str,
) -> Result<(), WsHandshakeFailure> {
    stream
        .write_all(request.as_bytes())
        .await
        .map_err(WsHandshakeFailure::io)
}

/// Reads one HTTP/1.1 response off `stream`, head and body.
///
/// The head is read a byte at a time on purpose. After a `101` the very next
/// bytes on the socket belong to the WebSocket stream, and a buffered read that
/// over-shot the `\r\n\r\n` would swallow the client's first frame — a failure
/// that only shows up as a mysteriously stalled test.
async fn read_http_response(stream: &mut TcpStream) -> Result<WsHttpResponse, WsHandshakeFailure> {
    let mut head = Vec::new();
    loop {
        let mut byte = [0u8; 1];
        let read = stream
            .read(&mut byte)
            .await
            .map_err(WsHandshakeFailure::io)?;
        if read == 0 {
            return Err(WsHandshakeFailure::Malformed(
                "connection closed before the response head was complete".to_string(),
            ));
        }
        head.push(byte[0]);
        if head.ends_with(b"\r\n\r\n") {
            break;
        }
    }

    let head_text = String::from_utf8_lossy(&head).to_string();
    let content_length = head_text
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            if name.trim().eq_ignore_ascii_case("content-length") {
                value.trim().parse::<usize>().ok()
            } else {
                None
            }
        })
        .unwrap_or(0);

    let mut body = vec![0u8; content_length];
    if content_length > 0 {
        stream
            .read_exact(&mut body)
            .await
            .map_err(WsHandshakeFailure::io)?;
    }

    parse_http_response(&head_text, &body)
}

fn parse_http_response(head: &str, body: &[u8]) -> Result<WsHttpResponse, WsHandshakeFailure> {
    let mut lines = head.lines();
    let status_line = lines.next().ok_or_else(|| {
        WsHandshakeFailure::Malformed("response had no status line".to_string())
    })?;

    let status = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| {
            WsHandshakeFailure::Malformed(format!("unparseable status line: {}", status_line))
        })?;

    let mut headers = HashMap::new();
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
        }
    }

    Ok(WsHttpResponse {
        status,
        headers,
        body: String::from_utf8_lossy(body).to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routing::websocket::register_ws_route;

    #[test]
    fn parses_a_response_head_and_body() {
        let parsed = parse_http_response(
            "HTTP/1.1 426 Upgrade Required\r\nContent-Length: 5\r\n",
            b"hello",
        )
        .unwrap();

        assert_eq!(parsed.status, 426);
        assert_eq!(parsed.header("content-length").as_deref(), Some("5"));
        assert_eq!(parsed.body, "hello");
    }

    #[tokio::test]
    async fn a_plain_get_to_a_ws_route_demands_an_upgrade() {
        register_ws_route("/ws/harness-probe", |_stream, _ctx| {
            Box::pin(std::future::pending::<()>())
        });

        let server = WsTestServer::start().await;
        let response = server.get("/ws/harness-probe").await;

        assert_eq!(response.status, 426);
        assert!(response.body.contains("Upgrade Required"), "{}", response.body);
    }

    #[tokio::test]
    async fn an_unsupported_version_is_refused_and_the_supported_one_is_advertised() {
        register_ws_route("/ws/harness-version", |_stream, _ctx| {
            Box::pin(std::future::pending::<()>())
        });

        let server = WsTestServer::start().await;
        let failure = server
            .try_connect_with_version("/ws/harness-version", "8")
            .await
            .expect_err("version 8 must not upgrade");

        assert_eq!(failure.status(), Some(426));
        assert_eq!(
            failure.header("sec-websocket-version").as_deref(),
            Some(TEST_WS_VERSION)
        );
    }

    #[tokio::test]
    async fn a_missing_key_is_a_bad_request_not_a_silent_101() {
        register_ws_route("/ws/harness-nokey", |_stream, _ctx| {
            Box::pin(std::future::pending::<()>())
        });

        let server = WsTestServer::start().await;
        let failure = server
            .try_connect_without_key("/ws/harness-nokey")
            .await
            .expect_err("a handshake with no key must not upgrade");

        assert_eq!(failure.status(), Some(400));
    }
}
