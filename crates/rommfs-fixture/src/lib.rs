//! Tiny local HTTP fixture server for tests — implements the documented
//! RomM contract subset with scripted failures and request counting.
//!
//! Used to prove behavior against the REAL client: "no download on listing",
//! "one download for concurrent reads", failure/retry paths — by counting
//! actual requests, not mocking internals.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

/// How the fixture answers one route.
#[derive(Clone, Debug)]
pub enum ResponseSpec {
    Json {
        status: u16,
        body: String,
    },
    JsonWithHeaders {
        status: u16,
        body: String,
        headers: Vec<(String, String)>,
    },
    /// Answer successive requests with these responses, repeating the final
    /// response after the sequence is exhausted.
    Sequence(Vec<ResponseSpec>),
    /// Accept the request and hold the connection without sending a response.
    /// The fixture shuts the connection down when dropped.
    Stall,
    /// Serve `bytes`; may truncate to `truncate_at` or stall after
    /// `stall_after_bytes` without sending more (for stall-timeout tests).
    Bytes {
        status: u16,
        bytes: Vec<u8>,
        truncate_at: Option<usize>,
        stall_after_bytes: Option<usize>,
    },
    /// Send response headers, then hold the content body until the test
    /// explicitly releases the barrier. This blocks a real HTTP body read.
    HeldBytes {
        status: u16,
        bytes: Vec<u8>,
        barrier: FixtureBodyBarrier,
    },
    /// Stateful subset of RomM's save API. Uploads receive the server's
    /// verified datetime filename tag and are visible to inventory/readback.
    RomMSaveUpload {
        status: u16,
        user_id: i64,
    },
    /// Capture a complete POST body, then block its response until explicitly
    /// released. The fixture has already received the immutable payload.
    HeldRomMSaveUpload {
        user_id: i64,
        barrier: FixtureBodyBarrier,
    },
    RomMSaveInventory,
    RomMSaveContent,
}

#[derive(Clone, Debug)]
pub struct FixtureBodyBarrier {
    started_tx: mpsc::Sender<()>,
    started_rx: Arc<Mutex<mpsc::Receiver<()>>>,
    release_tx: mpsc::Sender<()>,
    release_rx: Arc<Mutex<mpsc::Receiver<()>>>,
}

impl FixtureBodyBarrier {
    pub fn new() -> Self {
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        Self {
            started_tx,
            started_rx: Arc::new(Mutex::new(started_rx)),
            release_tx,
            release_rx: Arc::new(Mutex::new(release_rx)),
        }
    }

    pub fn wait_until_blocked(&self, timeout: Duration) -> bool {
        self.started_rx
            .lock()
            .unwrap()
            .recv_timeout(timeout)
            .is_ok()
    }

    pub fn release(&self) {
        let _ = self.release_tx.send(());
    }
}

impl Default for FixtureBodyBarrier {
    fn default() -> Self {
        Self::new()
    }
}

type RecordedAuth = HashMap<(String, String), Vec<Option<String>>>;

#[derive(Clone, Debug)]
pub struct CapturedRequest {
    pub method: String,
    pub target: String,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FixtureSaveRecord {
    pub id: i64,
    pub rom_id: i64,
    pub user_id: i64,
    pub file_name: String,
    pub file_size_bytes: usize,
    pub slot: String,
    pub bytes: Vec<u8>,
    pub created_at: String,
    pub updated_at: String,
}

/// Per-test scripted server. Drop/join stops it.
pub struct FixtureServer {
    base_url: String,
    state: Arc<FixtureState>,
    accept: Option<JoinHandle<()>>,
    conns: Arc<Mutex<Vec<JoinHandle<()>>>>,
}

struct FixtureState {
    /// (method,path-prefix) -> requests seen — proofs for "did not download".
    counts: Mutex<HashMap<(String, String), AtomicUsize>>,
    /// (method, path prefix) -> response spec. Longest matching prefix wins.
    routes: Mutex<Vec<(String, String, ResponseSpec)>>,
    /// Authorization header values seen per (method, path prefix), in order.
    auth_seen: Mutex<RecordedAuth>,
    requests_seen: Mutex<Vec<CapturedRequest>>,
    saves: Mutex<HashMap<i64, FixtureSaveRecord>>,
    next_save_id: AtomicUsize,
    shutdown: Arc<AtomicBool>,
}

impl FixtureServer {
    /// Start on an ephemeral localhost port.
    pub fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("fixture bind");
        listener.set_nonblocking(true).expect("fixture nonblocking");
        let addr = listener.local_addr().expect("fixture addr");
        let base_url = format!("http://{addr}");

        let state = Arc::new(FixtureState {
            counts: Mutex::new(HashMap::new()),
            routes: Mutex::new(Vec::new()),
            auth_seen: Mutex::new(HashMap::new()),
            requests_seen: Mutex::new(Vec::new()),
            saves: Mutex::new(HashMap::new()),
            next_save_id: AtomicUsize::new(30),
            shutdown: Arc::new(AtomicBool::new(false)),
        });
        let conns: Arc<Mutex<Vec<JoinHandle<()>>>> = Arc::new(Mutex::new(Vec::new()));

        let accept = {
            let state = Arc::clone(&state);
            let conns = Arc::clone(&conns);
            std::thread::spawn(move || loop {
                if state.shutdown.load(Ordering::SeqCst) {
                    break;
                }
                match listener.accept() {
                    Ok((stream, _)) => {
                        // Windows accepts inherit the listener's nonblocking
                        // flag; the handler relies on blocking I/O + timeouts.
                        let _ = stream.set_nonblocking(false);
                        let h = std::thread::spawn({
                            let state = Arc::clone(&state);
                            move || handle_conn(stream, &state)
                        });
                        conns.lock().unwrap().push(h);
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            })
        };

        Self {
            base_url,
            state,
            accept: Some(accept),
            conns,
        }
    }

    pub fn url(&self) -> &str {
        &self.base_url
    }

    /// Register/override a route: `on("GET", "/api/roms", spec)`.
    /// `path_prefix` is matched against the request target *including* any
    /// query string, so "/api/roms?" matches the collection only while
    /// "/api/roms/" matches member/content paths.
    pub fn on(&self, method: &str, path_prefix: &str, spec: ResponseSpec) {
        let key = (method.to_uppercase(), path_prefix.to_string());
        let mut routes = self.state.routes.lock().unwrap();
        routes.retain(|(m, p, _)| !(m == &key.0 && p == &key.1));
        routes.push((key.0.clone(), key.1.clone(), spec));
        drop(routes);
        self.state
            .counts
            .lock()
            .unwrap()
            .entry(key)
            .or_insert_with(|| AtomicUsize::new(0));
    }

    pub fn on_sequence(&self, method: &str, path_prefix: &str, specs: Vec<ResponseSpec>) {
        assert!(!specs.is_empty(), "response sequence must not be empty");
        self.on(method, path_prefix, ResponseSpec::Sequence(specs));
    }

    /// Requests seen for (method, path prefix).
    pub fn count(&self, method: &str, path_prefix: &str) -> usize {
        self.state
            .counts
            .lock()
            .unwrap()
            .get(&(method.to_uppercase(), path_prefix.to_string()))
            .map(|count| count.load(Ordering::SeqCst))
            .unwrap_or(0)
    }

    /// Count captured requests by their actual wire target, independent of
    /// the route prefix used to script a dynamic handler.
    pub fn count_requests(&self, method: &str, path_prefix: &str) -> usize {
        self.state
            .requests_seen
            .lock()
            .unwrap()
            .iter()
            .filter(|request| {
                request.method.eq_ignore_ascii_case(method)
                    && request.target.starts_with(path_prefix)
            })
            .count()
    }

    /// Authorization header values observed on (method, path prefix) hits.
    pub fn auth_headers(&self, method: &str, path_prefix: &str) -> Vec<Option<String>> {
        self.state
            .auth_seen
            .lock()
            .unwrap()
            .get(&(method.to_uppercase(), path_prefix.to_string()))
            .cloned()
            .unwrap_or_default()
    }

    /// Fully captured wire requests, including raw multipart body bytes.
    pub fn requests(&self) -> Vec<CapturedRequest> {
        self.state.requests_seen.lock().unwrap().clone()
    }

    /// Install the stateful save API routes. A non-success `upload_status`
    /// still stores the file first, modeling a lost response after acceptance.
    pub fn use_romm_save_store(&self, user_id: i64, upload_status: u16) {
        self.on("GET", "/api/saves?", ResponseSpec::RomMSaveInventory);
        self.on("GET", "/api/saves/", ResponseSpec::RomMSaveContent);
        self.on(
            "POST",
            "/api/saves?",
            ResponseSpec::RomMSaveUpload {
                status: upload_status,
                user_id,
            },
        );
    }

    pub fn saved_saves(&self) -> Vec<FixtureSaveRecord> {
        let mut saves = self
            .state
            .saves
            .lock()
            .unwrap()
            .values()
            .cloned()
            .collect::<Vec<_>>();
        saves.sort_by_key(|save| save.id);
        saves
    }

    /// Seed a remote history record without issuing an HTTP POST.
    pub fn seed_save(&self, save: FixtureSaveRecord) {
        self.state.saves.lock().unwrap().insert(save.id, save);
    }
}

impl Drop for FixtureServer {
    fn drop(&mut self) {
        self.state.shutdown.store(true, Ordering::SeqCst);
        if let Some(h) = self.accept.take() {
            let _ = h.join();
        }
        for h in self.conns.lock().unwrap().drain(..) {
            let _ = h.join();
        }
    }
}

fn handle_conn(mut stream: TcpStream, state: &FixtureState) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(10)));

    let Some(request) = read_request(&mut stream) else {
        return;
    };

    let method = request.method.to_uppercase();
    let target = request.target.clone();
    let auth = request.headers.get("authorization").cloned();
    state.requests_seen.lock().unwrap().push(request.clone());
    let matched = {
        let routes = state.routes.lock().unwrap();
        routes
            .iter()
            .filter(|(m, prefix, _)| *m == method && target.starts_with(prefix.as_str()))
            .max_by_key(|(_, prefix, _)| prefix.len())
            .map(|(m, p, spec)| (m.clone(), p.clone(), spec.clone()))
    };

    let Some((m, prefix, spec)) = matched else {
        let body = r#"{"detail":"fixture: no route"}"#;
        let _ = write_response_head(&mut stream, 404, "application/json", body.len());
        let _ = stream.write_all(body.as_bytes());
        return;
    };

    let request_index = state
        .counts
        .lock()
        .unwrap()
        .get(&(m.clone(), prefix.clone()))
        .map_or(0, |count| count.fetch_add(1, Ordering::SeqCst));
    state
        .auth_seen
        .lock()
        .unwrap()
        .entry((m, prefix))
        .or_default()
        .push(auth);

    let spec = match spec {
        ResponseSpec::Sequence(sequence) => sequence
            .get(request_index)
            .or_else(|| sequence.last())
            .cloned()
            .unwrap_or(ResponseSpec::Json {
                status: 500,
                body: r#"{"detail":"fixture: empty response sequence"}"#.into(),
            }),
        spec => spec,
    };
    match spec {
        ResponseSpec::Stall => {
            while !state.shutdown.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        ResponseSpec::Json { status, body } => {
            if write_response_head(&mut stream, status, "application/json", body.len()).is_err() {
                return;
            }
            let _ = stream.write_all(body.as_bytes());
        }
        ResponseSpec::Sequence(_) => {
            let body = r#"{"detail":"fixture: nested response sequence"}"#;
            let _ = write_response_head(&mut stream, 500, "application/json", body.len());
            let _ = stream.write_all(body.as_bytes());
        }
        ResponseSpec::JsonWithHeaders {
            status,
            body,
            headers,
        } => {
            if write_response_head_with_headers(
                &mut stream,
                status,
                "application/json",
                body.len(),
                &headers,
            )
            .is_err()
            {
                return;
            }
            let _ = stream.write_all(body.as_bytes());
        }
        ResponseSpec::Bytes {
            status,
            bytes,
            truncate_at,
            stall_after_bytes,
        } => {
            // Content-Length always declares the full body; truncation and
            // stalls happen on the wire only.
            if write_response_head(&mut stream, status, "application/octet-stream", bytes.len())
                .is_err()
            {
                return;
            }
            let send = truncate_at.map_or(bytes.len(), |n| n.min(bytes.len()));
            let stall_at = stall_after_bytes.map_or(send, |n| n.min(send));
            if stream.write_all(&bytes[..stall_at]).is_err() {
                return;
            }
            let _ = stream.flush();
            if stall_at < send {
                if stall_after_bytes.is_some() {
                    // Hold the transfer open ~30s in 1s chunks so a
                    // no-progress timeout can fire; bail early on shutdown.
                    for _ in 0..30 {
                        if state.shutdown.load(Ordering::SeqCst) {
                            return;
                        }
                        std::thread::sleep(Duration::from_secs(1));
                    }
                }
                let _ = stream.write_all(&bytes[stall_at..send]);
            }
        }
        ResponseSpec::HeldBytes {
            status,
            bytes,
            barrier,
        } => {
            if write_response_head(&mut stream, status, "application/octet-stream", bytes.len())
                .is_err()
            {
                return;
            }
            let _ = barrier.started_tx.send(());
            if barrier.release_rx.lock().unwrap().recv().is_err() {
                return;
            }
            let _ = stream.write_all(&bytes);
        }
        ResponseSpec::RomMSaveUpload { status, user_id } => {
            let Some(save) = store_uploaded_save(&request, user_id, &state.next_save_id) else {
                let body = r#"{"detail":"fixture: invalid save upload"}"#;
                let _ = write_response_head(&mut stream, 400, "application/json", body.len());
                let _ = stream.write_all(body.as_bytes());
                return;
            };
            state.saves.lock().unwrap().insert(save.id, save.clone());
            let body = if (200..300).contains(&status) {
                save_json(&save)
            } else {
                r#"{"detail":"fixture: request accepted before response loss"}"#.into()
            };
            if write_response_head(&mut stream, status, "application/json", body.len()).is_ok() {
                let _ = stream.write_all(body.as_bytes());
            }
        }
        ResponseSpec::HeldRomMSaveUpload { user_id, barrier } => {
            let _ = barrier.started_tx.send(());
            if barrier.release_rx.lock().unwrap().recv().is_err() {
                return;
            }
            let Some(save) = store_uploaded_save(&request, user_id, &state.next_save_id) else {
                let body = r#"{"detail":"fixture: invalid save upload"}"#;
                let _ = write_response_head(&mut stream, 400, "application/json", body.len());
                let _ = stream.write_all(body.as_bytes());
                return;
            };
            state.saves.lock().unwrap().insert(save.id, save.clone());
            let body = save_json(&save);
            if write_response_head(&mut stream, 201, "application/json", body.len()).is_ok() {
                let _ = stream.write_all(body.as_bytes());
            }
        }
        ResponseSpec::RomMSaveInventory => {
            let body = save_inventory_json(&request.target, &state.saves);
            if write_response_head(&mut stream, 200, "application/json", body.len()).is_ok() {
                let _ = stream.write_all(body.as_bytes());
            }
        }
        ResponseSpec::RomMSaveContent => {
            let Some(id) = request
                .target
                .strip_prefix("/api/saves/")
                .and_then(|path| path.split('/').next())
                .and_then(|id| id.parse::<i64>().ok())
            else {
                let _ = write_response_head(&mut stream, 404, "application/json", 0);
                return;
            };
            let stored = state.saves.lock().unwrap().get(&id).cloned();
            let Some(stored) = stored else {
                let _ = write_response_head(&mut stream, 404, "application/json", 0);
                return;
            };
            if write_response_head(
                &mut stream,
                200,
                "application/octet-stream",
                stored.bytes.len(),
            )
            .is_ok()
            {
                let _ = stream.write_all(&stored.bytes);
            }
        }
    }
}

fn store_uploaded_save(
    request: &CapturedRequest,
    user_id: i64,
    next_save_id: &AtomicUsize,
) -> Option<FixtureSaveRecord> {
    let rom_id = query_value(&request.target, "rom_id")?.parse().ok()?;
    let slot = query_value(&request.target, "slot")?;
    let (filename, bytes) = multipart_save_file(request)?;
    let (stem, extension) = filename.rsplit_once('.')?;
    let file_name = format!("{stem} [2026-10-05_12-34-56].{extension}");
    let id = next_save_id.fetch_add(1, Ordering::SeqCst) as i64 + 1;
    Some(FixtureSaveRecord {
        id,
        rom_id,
        user_id,
        file_name,
        file_size_bytes: bytes.len(),
        slot,
        bytes,
        created_at: "2026-10-05T12:34:56Z".into(),
        updated_at: "2026-10-05T12:34:56Z".into(),
    })
}

fn multipart_save_file(request: &CapturedRequest) -> Option<(String, Vec<u8>)> {
    let content_type = request.headers.get("content-type")?;
    let boundary = content_type
        .split(';')
        .map(str::trim)
        .find_map(|parameter| parameter.strip_prefix("boundary="))?;
    let boundary = format!("--{boundary}");
    let header_end = find_double_crlf(&request.body)?;
    let headers = String::from_utf8_lossy(&request.body[..header_end]);
    let filename = headers
        .lines()
        .find_map(|line| line.split_once("filename=\"").map(|(_, rest)| rest))?
        .split('"')
        .next()?
        .to_owned();
    let content_start = header_end + 4;
    let closing_boundary = format!("\r\n{boundary}--\r\n");
    let content_end = request.body[content_start..]
        .windows(closing_boundary.len())
        .position(|window| window == closing_boundary.as_bytes())?
        + content_start;
    Some((filename, request.body[content_start..content_end].to_vec()))
}

fn save_inventory_json(target: &str, saves: &Mutex<HashMap<i64, FixtureSaveRecord>>) -> String {
    let rom_id = query_value(target, "rom_id").and_then(|value| value.parse::<i64>().ok());
    let slot = query_value(target, "slot");
    let saves = saves.lock().unwrap();
    serde_json::Value::Array(
        saves
            .values()
            .filter(|save| {
                Some(save.rom_id) == rom_id && slot.as_deref() == Some(save.slot.as_str())
            })
            .map(save_json_value)
            .collect(),
    )
    .to_string()
}

fn save_json(save: &FixtureSaveRecord) -> String {
    save_json_value(save).to_string()
}

fn save_json_value(save: &FixtureSaveRecord) -> serde_json::Value {
    serde_json::json!({
        "id": save.id,
        "rom_id": save.rom_id,
        "user_id": save.user_id,
        "file_name": save.file_name,
        "file_size_bytes": save.file_size_bytes,
        "missing_from_fs": false,
        "created_at": save.created_at,
        "updated_at": save.updated_at,
        "emulator": "retroarch-gambatte",
        "slot": save.slot,
    })
}

fn query_value(target: &str, key: &str) -> Option<String> {
    target
        .split_once('?')?
        .1
        .split('&')
        .filter_map(|part| part.split_once('='))
        .find_map(|(name, value)| (name == key).then(|| value.to_owned()))
}

/// Read one HTTP request line, headers, and body according to Content-Length.
fn read_request(stream: &mut TcpStream) -> Option<CapturedRequest> {
    let mut buf = Vec::with_capacity(2048);
    let mut tmp = [0u8; 4096];
    let header_end;
    loop {
        match stream.read(&mut tmp) {
            Ok(0) => return None,
            Ok(n) => {
                buf.extend_from_slice(&tmp[..n]);
                if let Some(pos) = find_double_crlf(&buf) {
                    header_end = pos;
                    break;
                }
                if buf.len() > 64 * 1024 {
                    return None;
                }
            }
            Err(_) => return None,
        }
    }

    let head = String::from_utf8_lossy(&buf[..header_end]);
    let mut lines = head.split("\r\n");
    let request_line = lines.next()?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next()?.to_string();
    let target = parts.next()?.to_string();

    let mut content_length = 0usize;
    let mut headers = HashMap::new();
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            let name = name.trim().to_ascii_lowercase();
            let value = value.trim();
            if name == "content-length" {
                content_length = value.parse().unwrap_or(0);
            }
            headers.insert(name, value.to_string());
        }
    }

    // Drain the request body (best-effort; the socket closes after the
    // response anyway).
    let want = header_end + 4 + content_length;
    while buf.len() < want {
        match stream.read(&mut tmp) {
            Ok(0) => break,
            Ok(n) => buf.extend_from_slice(&tmp[..n]),
            Err(_) => break,
        }
    }

    let body_start = header_end + 4;
    Some(CapturedRequest {
        method,
        target,
        headers,
        body: buf[body_start..buf.len().min(want)].to_vec(),
    })
}

fn find_double_crlf(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

fn reason_of(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        301 => "Moved Permanently",
        302 => "Found",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        _ => "Status",
    }
}

fn write_response_head(
    stream: &mut TcpStream,
    status: u16,
    content_type: &str,
    content_length: usize,
) -> std::io::Result<()> {
    write_response_head_with_headers(stream, status, content_type, content_length, &[])
}

fn write_response_head_with_headers(
    stream: &mut TcpStream,
    status: u16,
    content_type: &str,
    content_length: usize,
    headers: &[(String, String)],
) -> std::io::Result<()> {
    let extra_headers = headers
        .iter()
        .map(|(name, value)| format!("{name}: {value}\r\n"))
        .collect::<String>();
    let head = format!(
        "HTTP/1.1 {status} {}\r\nContent-Type: {content_type}\r\nContent-Length: {content_length}\r\n{extra_headers}Connection: close\r\n\r\n",
        reason_of(status),
    );
    stream.write_all(head.as_bytes())
}

/// Builder for the catalogue payloads matching `.planning/API-CONTRACT.md`.
pub mod contract {
    use serde_json::{json, Value};

    /// `TokenResponse` JSON for POST /api/token.
    pub fn token_ok() -> String {
        json!({
            "access_token": "fixture-access-token",
            "token_type": "bearer",
            "expires": 3600,
            "refresh_token": "fixture-refresh-token",
            "refresh_expires": 86400,
        })
        .to_string()
    }

    /// Platform list JSON. Items are `(id, slug, fs_slug, name)`.
    pub fn platforms(items: &[(i64, &str, &str, &str)]) -> String {
        let arr: Vec<Value> = items
            .iter()
            .map(|(id, slug, fs_slug, name)| {
                json!({
                    "id": id,
                    "slug": slug,
                    "fs_slug": fs_slug,
                    "name": name,
                    "custom_name": null,
                    "rom_count": 0,
                })
            })
            .collect();
        Value::Array(arr).to_string()
    }

    /// One `items` page of GET /api/roms.
    pub fn roms_page(
        items: &[serde_json::Value],
        total: usize,
        limit: usize,
        offset: usize,
    ) -> String {
        json!({
            "items": items,
            "total": total,
            "limit": limit,
            "offset": offset,
        })
        .to_string()
    }

    /// A SimpleRomSchema-shaped JSON object for a single-file ROM.
    pub fn rom(
        id: i64,
        platform_fs_slug: &str,
        file_name: &str,
        size: u64,
        sha1: &str,
    ) -> serde_json::Value {
        json!({
            "id": id,
            "platform_fs_slug": platform_fs_slug,
            "platform_slug": platform_fs_slug,
            "fs_name": file_name,
            "fs_size_bytes": size,
            "has_simple_single_file": true,
            "has_nested_single_file": false,
            "has_multiple_files": false,
            "missing_from_fs": false,
            "is_physical": true,
            "updated_at": "2026-10-01T00:00:00",
            "files": [rom_file(id, file_name, size, Some(sha1))],
        })
    }

    /// A multi-file (unsupported) ROM shape.
    pub fn rom_multi(
        id: i64,
        platform_fs_slug: &str,
        names: &[&str],
        sizes: &[u64],
    ) -> serde_json::Value {
        let files: Vec<Value> = names
            .iter()
            .zip(sizes.iter())
            .enumerate()
            .map(|(i, (n, s))| rom_file(id * 1000 + i as i64, n, *s, None))
            .collect();
        json!({
            "id": id,
            "platform_fs_slug": platform_fs_slug,
            "platform_slug": platform_fs_slug,
            "fs_name": names.first().copied().unwrap_or_default(),
            "fs_size_bytes": sizes.iter().sum::<u64>(),
            "has_simple_single_file": false,
            "has_nested_single_file": false,
            "has_multiple_files": true,
            "missing_from_fs": false,
            "is_physical": true,
            "updated_at": "2026-10-01T00:00:00",
            "files": files,
        })
    }

    fn rom_file(id: i64, file_name: &str, size: u64, sha1: Option<&str>) -> Value {
        json!({
            "id": id,
            "file_name": file_name,
            "file_size_bytes": size,
            "last_modified": "2026-10-01T00:00:00",
            "crc_hash": null,
            "md5_hash": null,
            "sha1_hash": sha1,
            "is_top_level": true,
        })
    }
}
