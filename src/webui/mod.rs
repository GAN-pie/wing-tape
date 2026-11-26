//! Minimal HTTP control surface for the pipeline.
//!
//! The server exposes only the reversible actions (Pause/Resume) and a
//! read-only status view. It is a peer producer on the pipeline event bus: a
//! request translates into a [`SystemEvent::User`] message and the orchestrator
//! remains the single writer of the pipeline state.
//!
//! Shutdown is cooperative: the accept loop polls an [`AtomicBool`] shared with
//! `main`, so the owner of the process stays in charge of the lifecycle and the
//! actor never blocks the orchestrator join on a lingering `event_tx` clone.

use crate::pipeline::{PipelineState, PipelineStatus, SharedStatus, SystemEvent, UserCommand};
use std::io;
use std::net::{SocketAddr, TcpListener};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

/// How long the accept loop waits for a request before re-checking shutdown.
const POLL_INTERVAL: Duration = Duration::from_millis(100);

/// HTTP server bound to `bind` that relays Pause/Resume onto the event bus.
pub struct WebControlActor {
    bind: SocketAddr,
    status: SharedStatus,
    event_tx: mpsc::Sender<SystemEvent>,
    shutdown: Arc<AtomicBool>,
}

impl WebControlActor {
    pub fn new(
        bind: SocketAddr,
        status: SharedStatus,
        event_tx: mpsc::Sender<SystemEvent>,
        shutdown: Arc<AtomicBool>,
    ) -> Self {
        Self {
            bind,
            status,
            event_tx,
            shutdown,
        }
    }

    /// Binds synchronously so configuration errors surface before the thread
    /// starts, then serves requests until `shutdown` is set.
    pub fn spawn(self) -> io::Result<WebHandle> {
        let listener = TcpListener::bind(self.bind)?;
        let addr = listener.local_addr()?;
        let server = Server::from_listener(listener, None)
            .map_err(|err| io::Error::other(err.to_string()))?;

        let join = thread::Builder::new()
            .name("web-control".to_string())
            .spawn(move || self.run(server))?;

        Ok(WebHandle { addr, join })
    }

    fn run(self, server: Server) {
        while !self.shutdown.load(Ordering::Relaxed) {
            match server.recv_timeout(POLL_INTERVAL) {
                Ok(Some(request)) => self.handle(request),
                Ok(None) => {}
                Err(err) => {
                    if !self.shutdown.load(Ordering::Relaxed) {
                        eprintln!("web control server error: {err}");
                    }
                    break;
                }
            }
        }
    }

    fn handle(&self, request: Request) {
        let method = request.method().clone();
        let url = request.url().to_string();

        match route(&method, &url) {
            Route::Index => self.respond(
                request,
                200,
                "text/html; charset=utf-8",
                INDEX_HTML.to_string(),
            ),
            Route::Status => {
                let body = render_status(&self.snapshot());
                self.respond(request, 200, "application/json", body);
            }
            Route::Pause => self.handle_command(request, UserCommand::Pause),
            Route::Resume => self.handle_command(request, UserCommand::Resume),
            Route::MethodNotAllowed => self.respond(
                request,
                405,
                "text/plain; charset=utf-8",
                "method not allowed\n".to_string(),
            ),
            Route::NotFound => self.respond(
                request,
                404,
                "text/plain; charset=utf-8",
                "not found\n".to_string(),
            ),
        }
    }

    fn handle_command(&self, request: Request, command: UserCommand) {
        let status = self.snapshot();
        if matches!(status.state, PipelineState::Stopped | PipelineState::Failed) {
            let state = state_name(status.state);
            let body = format!("{{\"error\":\"pipeline is {state}\",\"state\":\"{state}\"}}");
            self.respond(request, 409, "application/json", body);
            return;
        }

        match self.event_tx.send(SystemEvent::User(command)) {
            Ok(()) => {
                // The orchestrator applies the transition asynchronously. Return
                // the current snapshot and let the UI poll `/api/status`.
                let body = render_status(&self.snapshot());
                self.respond(request, 202, "application/json", body);
            }
            Err(_) => self.respond(
                request,
                503,
                "application/json",
                "{\"error\":\"pipeline unavailable\"}".to_string(),
            ),
        }
    }

    /// Clones the snapshot while holding the lock, then releases it. The caller
    /// writes the response without holding the mutex, so a slow client cannot
    /// stall the orchestrator.
    fn snapshot(&self) -> PipelineStatus {
        match self.status.lock() {
            Ok(guard) => guard.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }

    fn respond(&self, request: Request, code: u16, content_type: &'static str, body: String) {
        let content_type_header = Header::from_bytes(&b"Content-Type"[..], content_type.as_bytes())
            .expect("static content type is a valid header");
        let mut response = Response::from_string(body)
            .with_status_code(StatusCode(code))
            .with_header(content_type_header);

        if content_type.starts_with("application/json") {
            if let Ok(header) = Header::from_bytes(&b"Cache-Control"[..], &b"no-store"[..]) {
                response = response.with_header(header);
            }
        }

        let _ = request.respond(response);
    }
}

/// Handle to a spawned web control server.
pub struct WebHandle {
    addr: SocketAddr,
    join: JoinHandle<()>,
}

impl WebHandle {
    /// Address actually bound. Useful when the caller requested port `0`.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Waits for the accept loop to exit. The caller should set the shared
    /// shutdown flag first.
    pub fn join(self) -> thread::Result<()> {
        self.join.join()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Route {
    Index,
    Status,
    Pause,
    Resume,
    MethodNotAllowed,
    NotFound,
}

fn route(method: &Method, url: &str) -> Route {
    let path = url.split('?').next().unwrap_or(url);

    match path {
        "/" => {
            if method == &Method::Get {
                Route::Index
            } else {
                Route::MethodNotAllowed
            }
        }
        "/api/status" => {
            if method == &Method::Get {
                Route::Status
            } else {
                Route::MethodNotAllowed
            }
        }
        "/api/pause" => {
            if method == &Method::Post {
                Route::Pause
            } else {
                Route::MethodNotAllowed
            }
        }
        "/api/resume" => {
            if method == &Method::Post {
                Route::Resume
            } else {
                Route::MethodNotAllowed
            }
        }
        _ => Route::NotFound,
    }
}

fn state_name(state: PipelineState) -> &'static str {
    match state {
        PipelineState::Running => "running",
        PipelineState::Paused => "paused",
        PipelineState::Stopped => "stopped",
        PipelineState::Failed => "failed",
    }
}

/// Milliseconds since the Unix epoch, UTC. `0` means the epoch and is also the
/// fallback for times before the epoch.
pub fn epoch_millis(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

/// Inverse of [`epoch_millis`].
pub fn system_time_from_epoch_millis(unix_ms: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_millis(unix_ms)
}

fn render_status(status: &PipelineStatus) -> String {
    let mut out = String::with_capacity(256);
    out.push_str("{\"state\":");
    push_json_string(&mut out, state_name(status.state));
    out.push_str(",\"started_at\":");
    out.push_str(&epoch_millis(status.started_at).to_string());
    out.push_str(",\"updated_at\":");
    out.push_str(&epoch_millis(status.updated_at).to_string());
    out.push_str(",\"last_prediction\":");

    match &status.last_prediction {
        Some(detection) => {
            out.push_str("{\"class_id\":");
            out.push_str(&detection.class_id.to_string());
            out.push_str(",\"probability\":");
            push_json_probability(&mut out, detection.probability);
            out.push_str(",\"detected_at\":");
            out.push_str(&epoch_millis(detection.detected_at).to_string());
            out.push_str(",\"label\":");
            match &detection.label {
                Some(label) => push_json_string(&mut out, label),
                None => out.push_str("null"),
            }
            out.push('}');
        }
        None => out.push_str("null"),
    }

    out.push_str(",\"last_error\":");
    match &status.last_error {
        Some(error) => push_json_string(&mut out, error),
        None => out.push_str("null"),
    }
    out.push('}');
    out
}

fn push_json_probability(out: &mut String, probability: f32) {
    if probability.is_finite() {
        out.push_str(&probability.to_string());
    } else {
        out.push_str("null");
    }
}

fn push_json_string(out: &mut String, value: &str) {
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

const INDEX_HTML: &str = r##"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Wing-Tape</title>
<style>
:root { color-scheme: light dark; }
body {
  font-family: system-ui, -apple-system, Segoe UI, sans-serif;
  max-width: 40rem;
  margin: 4rem auto;
  padding: 0 1rem;
  line-height: 1.5;
}
h1 { font-size: 1.4rem; }
.status { font-size: 1.15rem; margin: 1rem 0; }
.state { font-weight: 600; text-transform: capitalize; }
.buttons { display: flex; gap: 0.75rem; margin: 1.5rem 0; }
button {
  font: inherit;
  padding: 0.6rem 1.2rem;
  border-radius: 0.5rem;
  border: 1px solid #8884;
  background: transparent;
  cursor: pointer;
}
button:disabled { opacity: 0.45; cursor: not-allowed; }
.detail { font-size: 0.9rem; opacity: 0.8; margin: 0.35rem 0; }
.error { color: #b00020; }
</style>
</head>
<body>
<h1>Wing-Tape</h1>
<p class="status">State: <span id="state" class="state">loading</span></p>
<div class="buttons">
  <button id="pause">Pause</button>
  <button id="resume">Resume</button>
</div>
<p id="prediction" class="detail"></p>
<p id="error" class="detail error"></p>
<script>
const stateEl = document.getElementById('state');
const predictionEl = document.getElementById('prediction');
const errorEl = document.getElementById('error');
const pauseBtn = document.getElementById('pause');
const resumeBtn = document.getElementById('resume');
let busy = false;

function render(status) {
  stateEl.textContent = status.state;
  pauseBtn.disabled = busy || status.state !== 'running';
  resumeBtn.disabled = busy || status.state !== 'paused';

  if (status.last_prediction) {
    const prediction = status.last_prediction;
    const label = prediction.label ? prediction.label : 'class ' + prediction.class_id;
    predictionEl.textContent = 'Last detection: ' + label +
      ' (' + (prediction.probability * 100).toFixed(1) + '%)';
  } else {
    predictionEl.textContent = 'No detection yet.';
  }

  errorEl.textContent = status.last_error ? 'Last error: ' + status.last_error : '';
}

async function refresh() {
  try {
    const response = await fetch('/api/status', { cache: 'no-store' });
    if (response.ok) render(await response.json());
  } catch (_) {
    // Server unreachable; keep the last rendered view.
  }
}

async function command(path) {
  busy = true;
  pauseBtn.disabled = true;
  resumeBtn.disabled = true;
  try {
    await fetch(path, { method: 'POST' });
  } catch (_) {
    // Fall through to a refresh; the next poll reports the real state.
  } finally {
    busy = false;
  }
  await refresh();
}

pauseBtn.addEventListener('click', () => command('/api/pause'));
resumeBtn.addEventListener('click', () => command('/api/resume'));

refresh();
setInterval(refresh, 1000);
</script>
</body>
</html>
"##;

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::sync::Mutex;

    fn test_actor() -> (
        WebHandle,
        mpsc::Receiver<SystemEvent>,
        SharedStatus,
        Arc<AtomicBool>,
    ) {
        let (event_tx, event_rx) = mpsc::channel();
        let status: SharedStatus = Arc::new(Mutex::new(PipelineStatus::new()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let actor = WebControlActor::new(
            SocketAddr::from(([127, 0, 0, 1], 0)),
            Arc::clone(&status),
            event_tx,
            Arc::clone(&shutdown),
        );
        let handle = actor.spawn().expect("bind ephemeral port");
        (handle, event_rx, status, shutdown)
    }

    fn http_request(addr: SocketAddr, method: &str, path: &str) -> String {
        let mut stream = std::net::TcpStream::connect(addr).expect("connect to test server");
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("set read timeout");
        write!(
            stream,
            "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n"
        )
        .expect("write request");

        let mut response = String::new();
        match stream.read_to_string(&mut response) {
            Ok(_) => {}
            Err(err)
                if matches!(
                    err.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(err) => panic!("read response: {err}"),
        }
        response
    }

    fn stop(handle: WebHandle, shutdown: &Arc<AtomicBool>) {
        shutdown.store(true, Ordering::SeqCst);
        let _ = handle.join();
    }

    #[test]
    fn routes_only_expose_pause_and_resume() {
        assert_eq!(route(&Method::Get, "/"), Route::Index);
        assert_eq!(route(&Method::Get, "/?refresh=1"), Route::Index);
        assert_eq!(route(&Method::Get, "/api/status"), Route::Status);
        assert_eq!(route(&Method::Post, "/api/pause"), Route::Pause);
        assert_eq!(route(&Method::Post, "/api/resume"), Route::Resume);

        assert_eq!(route(&Method::Get, "/api/pause"), Route::MethodNotAllowed);
        assert_eq!(route(&Method::Post, "/api/stop"), Route::NotFound);
        assert_eq!(route(&Method::Post, "/api/shutdown"), Route::NotFound);
    }

    #[test]
    fn epoch_millis_round_trip() {
        let unix_ms = 1_700_000_000_123u64;
        assert_eq!(
            epoch_millis(system_time_from_epoch_millis(unix_ms)),
            unix_ms
        );
    }

    #[test]
    fn epoch_millis_before_epoch_is_zero() {
        let before = UNIX_EPOCH - Duration::from_millis(1);
        assert_eq!(epoch_millis(before), 0);
    }

    #[test]
    fn status_json_escapes_labels_and_errors() {
        let mut status = PipelineStatus::new();
        status.last_error = Some("quote \" slash \\ newline \n".to_string());
        let json = render_status(&status);

        assert!(json.contains("\"last_error\":\"quote \\\" slash \\\\ newline \\n\""));
        assert!(json.contains("\"last_prediction\":null"));
        assert!(json.contains("\"state\":\"running\""));
    }

    #[test]
    fn status_json_includes_detection_fields() {
        let mut status = PipelineStatus::new();
        status.last_prediction = Some(crate::pipeline::Detection {
            class_id: 7,
            probability: 0.625,
            detected_at: system_time_from_epoch_millis(1_700_000_000_000),
            label: Some("American Robin".to_string()),
        });

        let json = render_status(&status);
        assert!(json.contains("\"class_id\":7"));
        assert!(json.contains("\"probability\":0.625"));
        assert!(json.contains("\"detected_at\":1700000000000"));
        assert!(json.contains("\"label\":\"American Robin\""));
    }

    #[test]
    fn status_endpoint_returns_json() {
        let (handle, _events, _status, shutdown) = test_actor();
        let response = http_request(handle.addr(), "GET", "/api/status");

        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        assert!(response.contains("\"state\":\"running\""), "{response}");
        assert!(response.contains("\"started_at\":"), "{response}");

        stop(handle, &shutdown);
    }

    #[test]
    fn index_endpoint_serves_html() {
        let (handle, _events, _status, shutdown) = test_actor();
        let response = http_request(handle.addr(), "GET", "/");

        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        assert!(response.contains("Wing-Tape"), "{response}");
        assert!(response.contains("/api/pause"), "{response}");

        stop(handle, &shutdown);
    }

    #[test]
    fn join_releases_the_event_sender() {
        let (handle, events, _status, shutdown) = test_actor();

        shutdown.store(true, Ordering::SeqCst);
        let _ = handle.join();

        // The orchestrator relies on this: once the web actor is joined, its
        // event_tx clone must be gone so the event channel can close.
        assert!(matches!(
            events.recv_timeout(Duration::from_millis(100)),
            Err(mpsc::RecvTimeoutError::Disconnected)
        ));
    }

    #[test]
    fn pause_endpoint_injects_command() {
        let (handle, events, _status, shutdown) = test_actor();
        let response = http_request(handle.addr(), "POST", "/api/pause");

        assert!(response.starts_with("HTTP/1.1 202"), "{response}");
        match events
            .recv_timeout(Duration::from_secs(1))
            .expect("pause command")
        {
            SystemEvent::User(UserCommand::Pause) => {}
            other => panic!("unexpected event: {other:?}"),
        }

        stop(handle, &shutdown);
    }

    #[test]
    fn terminal_state_rejects_commands() {
        let (handle, events, status, shutdown) = test_actor();
        status.lock().unwrap().state = PipelineState::Failed;

        let response = http_request(handle.addr(), "POST", "/api/resume");

        assert!(response.starts_with("HTTP/1.1 409"), "{response}");
        assert!(matches!(events.try_recv(), Err(mpsc::TryRecvError::Empty)));

        stop(handle, &shutdown);
    }

    #[test]
    fn rejects_wrong_method_and_unknown_routes() {
        let (handle, _events, _status, shutdown) = test_actor();
        let addr = handle.addr();

        let wrong_method = http_request(addr, "GET", "/api/pause");
        assert!(wrong_method.starts_with("HTTP/1.1 405"), "{wrong_method}");

        let unknown = http_request(addr, "GET", "/nope");
        assert!(unknown.starts_with("HTTP/1.1 404"), "{unknown}");

        stop(handle, &shutdown);
    }
}
