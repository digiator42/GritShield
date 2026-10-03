use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::sync::broadcast;

/// The number of frames a slow SSE consumer may fall behind before frames are dropped.
///
/// Server-Sent Events is an at-most-once, best-effort channel. Dropping stale
/// frames keeps a stalled socket from pinning unbounded memory in the fan-out
/// buffer.
const BROADCAST_CAPACITY: usize = 256;

/// The default interval between SSE comment pings.
///
/// Idle proxies and load balancers frequently close connections that stay
/// silent for too long. A comment frame (`: keep-alive`) resets those timers
/// without being surfaced to the `EventSource` API on the client.
pub const DEFAULT_KEEP_ALIVE: Duration = Duration::from_secs(15);

/// A live, long-lived Server-Sent Events response body.
///
/// `SseStream` is deliberately built on a [`broadcast::broadcast`] channel so
/// that it stays [`Clone`], matching the `ResponseBody` contract. A handler
/// creates the stream, hands ownership to [`Response::sse`](crate::http::Response::sse),
/// and any number of unrelated tasks can later push frames onto it by cloning
/// the sender.
#[derive(Clone)]
pub struct SseStream {
    sender: broadcast::Sender<String>,
    /// Frames that must reach the wire before any broadcast frame.
    ///
    /// This solves the subscribe-after-send race: an MCP server has to emit
    /// its `endpoint` event before the client can learn the session id it
    /// needs for the follow-up POST, so that frame cannot go through the
    /// broadcast channel.
    initial: Vec<String>,
    keep_alive: Duration,
}

impl SseStream {
    /// Create a new stream with the default pinger interval.
    pub fn new() -> Self {
        Self::with_keep_alive(DEFAULT_KEEP_ALIVE)
    }

    /// Create a new stream with an explicit pinger interval.
    pub fn with_keep_alive(keep_alive: Duration) -> Self {
        let (sender, _receiver) = broadcast::channel(BROADCAST_CAPACITY);
        Self {
            sender,
            initial: Vec::new(),
            keep_alive,
        }
    }

    /// Queue a frame that is guaranteed to be written first.
    ///
    /// Used for the transport handshake, where the payload must reach the
    /// client before it is able to issue any further request.
    pub fn queue_initial(&mut self, event: &str, data: &str) -> &mut Self {
        self.initial.push(frame(event, data));
        self
    }

    /// Broadcast a frame to every attached consumer.
    ///
    /// Returns the number of live consumers. `Ok(0)` means nobody is listening
    /// yet, which is normal for a notification raised during startup.
    pub fn send(&self, event: &str, data: &str) -> Result<usize, String> {
        self.sender.send(frame(event, data)).map_err(|e| e.to_string())
    }

    /// Broadcast a frame whose payload is already a JSON document.
    pub fn send_json(&self, event: &str, data: &serde_json::Value) -> Result<usize, String> {
        self.send(event, &data.to_string())
    }

    /// Number of consumers currently attached.
    pub fn listeners(&self) -> usize {
        self.sender.receiver_count()
    }

    /// Attach a new consumer.
    ///
    /// Only frames sent after subscribing are observed, so callers that need
    /// the handshake frames should rely on [`queue_initial`](Self::queue_initial)
    /// rather than racing the subscription.
    pub fn subscribe(&self) -> broadcast::Receiver<String> {
        self.sender.subscribe()
    }

    pub fn keep_alive(&self) -> Duration {
        self.keep_alive
    }
}

impl Default for SseStream {
    fn default() -> Self {
        Self::new()
    }
}

/// Encode a single SSE frame.
///
/// Per the WHATWG event-stream grammar, every field is terminated by a bare LF
/// (not CRLF) and the frame ends with a blank line. Multi-line data payloads are
/// split across repeated `data:` fields as required.
pub fn frame(event: &str, data: &str) -> String {
    let mut out = String::with_capacity(data.len() + event.len() + 24);

    if !event.is_empty() {
        out.push_str("event: ");
        // An event name containing a newline would terminate the field early
        // and let an attacker inject arbitrary stream fields.
        out.push_str(&sanitize_field(event));
        out.push('\n');
    }

    for line in data.split('\n') {
        out.push_str("data: ");
        out.push_str(line);
        out.push('\n');
    }

    out.push('\n');
    out
}

/// Strip CR/LF so a field value can never break out of its SSE line.
fn sanitize_field(value: &str) -> String {
    value.chars().filter(|c| *c != '\n' && *c != '\r').collect()
}

/// Write the stream head, the queued handshake frames, then pump live frames
/// until every producer has dropped the stream.
///
/// The caller must have already written the response head (status line plus
/// headers) with no `Content-Length`: an SSE body has no known length, so the
/// body legitimately terminates when the connection closes.
pub async fn pump<W>(
    writer: &mut W,
    stream: SseStream,
    keep_alive: Duration,
) -> std::io::Result<()>
where
    W: AsyncWriteExt + Unpin,
{
    let SseStream {
        sender,
        initial,
        keep_alive: _,
    } = stream;

    for frame in &initial {
        writer.write_all(frame.as_bytes()).await?;
    }
    writer.flush().await?;

    let mut receiver = sender.subscribe();
    // Drop our own sender before looping. If it stayed alive, the channel could
    // never report `Closed` and the pump would never learn the session ended —
    // this clone is the only strong reference the pump itself holds, so the
    // session's copy becomes the sole owner.
    drop(sender);

    let mut ticker = tokio::time::interval(keep_alive);
    // The first tick resolves immediately; skip it so we never emit a ping
    // before any real frame has had a chance to arrive.
    ticker.tick().await;

    loop {
        tokio::select! {
            received = receiver.recv() => match received {
                Ok(frame) => {
                    writer.write_all(frame.as_bytes()).await?;
                    writer.flush().await?;
                }
                // A consumer that fell too far behind loses frames rather than
                // stalling the writer; keep the stream alive.
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                // The last producer went away. The owning `Response` is dropped
                // before this pump starts, so reaching here means the session
                // was genuinely terminated.
                Err(broadcast::error::RecvError::Closed) => break,
            },
            _ = ticker.tick() => {
                writer.write_all(b": keep-alive\n\n").await?;
                writer.flush().await?;
            }
        }
    }

    Ok(())
}