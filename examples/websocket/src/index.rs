use gritshield::prelude::*;
use gritshield::http::response::Response;
use gritshield::routing::engine::{Router, RequestContext};
use gritshield::http::HttpMethod;

pub fn register_index_routes(router: &mut Router) {
    router.add_route(HttpMethod::GET, "/", index_handler, None);
}

async fn index_handler(_ctx: RequestContext) -> Response {
    let html = r#"<!DOCTYPE html>
<html>
  <head>
    <meta charset="utf-8" />
    <title>GritShield WebSocket</title>
    <style>
      body { font-family: monospace; padding: 2rem; background: #111827; color: #e5e7eb; }
      pre { background: #1f2937; padding: 1rem; border-radius: 4px; height: 300px; overflow-y: auto; }
      .sent { color: #34d399; }
      input { width: 60%; padding: 0.5rem; }
      button { padding: 0.5rem 1rem; margin: 0.25rem; }
      select { padding: 0.5rem; }
      .panel { margin: 1rem 0; padding: 1rem; background: #1f2937; border-radius: 4px; }
    </style>
  </head>
  <body>
    <h3>WebSocket Echo Demo</h3>
    <div class="panel">
      <label>Endpoint: 
        <select id="endpoint">
          <option value="/ws/echo">Echo</option>
          <option value="/ws/broadcast">Broadcast</option>
          <option value="/ws/room/general">Room: general</option>
          <option value="/ws/room/random">Room: random</option>
        </select>
      </label>
      <button onclick="connect()">Connect</button>
      <button onclick="disconnect()">Disconnect</button>
    </div>
    <pre id="log"></pre>
    <input id="msg" placeholder="type a message" />
    <input id="user" placeholder="username" value="user" style="width: 15%;" />
    <button onclick="send()">Send</button>
    <script>
      const logEl = document.getElementById('log');
      const input = document.getElementById('msg');
      const userInput = document.getElementById('user');
      const endpointSel = document.getElementById('endpoint');
      let ws = null;
      function log(s) {
        const line = document.createElement('span');
        if (s.includes('[send]')) line.className = 'sent';
        line.textContent = s + '\n';
        logEl.appendChild(line);
        logEl.scrollTop = logEl.scrollHeight; 
      }
      
      function connect() {
        if (ws) ws.close();
        const endpoint = endpointSel.value;
        ws = new WebSocket('ws://' + location.host + endpoint);
        ws.onopen = () => log('[open] ' + endpoint);
        ws.onmessage = (e) => log('[recv] ' + e.data);
        ws.onclose = () => log('[close]');
        ws.onerror = (e) => log('[error]');
      }
      
      function disconnect() { if (ws) ws.close(); }
      
      window.send = () => { 
        if (ws && ws.readyState === 1) { 
          const payload = JSON.stringify({ 
            user: userInput.value, 
            text: input.value,
            room: null 
          });
          ws.send(payload); 
          log('[send] ' + payload); 
          input.value = ''; 
        } 
      };
    </script>
  </body>
</html>"#;
    Response::new(200, gritshield::security::xss::Sanitizer::trust(html))
}