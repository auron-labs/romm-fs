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
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

/// How the fixture answers one route.
#[derive(Clone, Debug)]
pub enum ResponseSpec {
    Json {
        status: u16,
        body: String,
    },
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
}

type RecordedAuth = HashMap<(String, String), Vec<Option<String>>>;

/// Per-test scripted server. Drop/join stops it.
pub struct FixtureServer {
    base_url: String,
    /// (method,path-prefix) -> requests seen — proofs for "did not download".
    counts: Arc<Mutex<HashMap<(String, String), AtomicUsize>>>,
    /// (method, path prefix) -> response spec. First prefix match wins.
    routes: Arc<Mutex<Vec<(String, String, ResponseSpec)>>>,
    /// Authorization header values seen per (method, path prefix), in order.
    auth_seen: Arc<Mutex<RecordedAuth>>,
    shutdown: Arc<AtomicBool>,
    accept: Option<JoinHandle<()>>,
    conns: Arc<Mutex<Vec<JoinHandle<()>>>>,
}

impl FixtureServer {
    /// Start on an ephemeral localhost port.
    pub fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("fixture bind");
        listener.set_nonblocking(true).expect("fixture nonblocking");
        let addr = listener.local_addr().expect("fixture addr");
        let base_url = format!("http://{addr}");

        let counts: Arc<Mutex<HashMap<(String, String), AtomicUsize>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let routes: Arc<Mutex<Vec<(String, String, ResponseSpec)>>> =
            Arc::new(Mutex::new(Vec::new()));
        let auth_seen: Arc<Mutex<RecordedAuth>> = Arc::new(Mutex::new(HashMap::new()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let conns: Arc<Mutex<Vec<JoinHandle<()>>>> = Arc::new(Mutex::new(Vec::new()));

        let accept = {
            let counts = Arc::clone(&counts);
            let routes = Arc::clone(&routes);
            let auth_seen = Arc::clone(&auth_seen);
            let shutdown = Arc::clone(&shutdown);
            let conns = Arc::clone(&conns);
            std::thread::spawn(move || loop {
                if shutdown.load(Ordering::SeqCst) {
                    break;
                }
                match listener.accept() {
                    Ok((stream, _)) => {
                        // Windows accepts inherit the listener's nonblocking
                        // flag; the handler relies on blocking I/O + timeouts.
                        let _ = stream.set_nonblocking(false);
                        let h = std::thread::spawn({
                            let counts = Arc::clone(&counts);
                            let routes = Arc::clone(&routes);
                            let auth_seen = Arc::clone(&auth_seen);
                            let shutdown = Arc::clone(&shutdown);
                            move || handle_conn(stream, &routes, &counts, &auth_seen, &shutdown)
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
            counts,
            routes,
            auth_seen,
            shutdown,
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
        let mut routes = self.routes.lock().unwrap();
        routes.retain(|(m, p, _)| !(m == &key.0 && p == &key.1));
        routes.push((key.0.clone(), key.1.clone(), spec));
        drop(routes);
        self.counts
            .lock()
            .unwrap()
            .entry(key)
            .or_insert_with(|| AtomicUsize::new(0));
    }

    /// Requests seen for (method, path prefix).
    pub fn count(&self, method: &str, path_prefix: &str) -> usize {
        self.counts
            .lock()
            .unwrap()
            .get(&(method.to_uppercase(), path_prefix.to_string()))
            .map(|c| c.load(Ordering::SeqCst))
            .unwrap_or(0)
    }

    /// Authorization header values observed on (method, path prefix) hits.
    pub fn auth_headers(&self, method: &str, path_prefix: &str) -> Vec<Option<String>> {
        self.auth_seen
            .lock()
            .unwrap()
            .get(&(method.to_uppercase(), path_prefix.to_string()))
            .cloned()
            .unwrap_or_default()
    }
}

impl Drop for FixtureServer {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        if let Some(h) = self.accept.take() {
            let _ = h.join();
        }
        for h in self.conns.lock().unwrap().drain(..) {
            let _ = h.join();
        }
    }
}

fn handle_conn(
    mut stream: TcpStream,
    routes: &Mutex<Vec<(String, String, ResponseSpec)>>,
    counts: &Mutex<HashMap<(String, String), AtomicUsize>>,
    auth_seen: &Mutex<RecordedAuth>,
    shutdown: &AtomicBool,
) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(10)));

    let Some((method, target, auth)) = read_request(&mut stream) else {
        return;
    };

    let method = method.to_uppercase();
    let matched = {
        let routes = routes.lock().unwrap();
        routes
            .iter()
            .find(|(m, prefix, _)| *m == method && target.starts_with(prefix.as_str()))
            .map(|(m, p, spec)| (m.clone(), p.clone(), spec.clone()))
    };

    let Some((m, prefix, spec)) = matched else {
        let body = r#"{"detail":"fixture: no route"}"#;
        let _ = write_response_head(&mut stream, 404, "application/json", body.len());
        let _ = stream.write_all(body.as_bytes());
        return;
    };

    if let Some(c) = counts.lock().unwrap().get(&(m.clone(), prefix.clone())) {
        c.fetch_add(1, Ordering::SeqCst);
    }
    auth_seen
        .lock()
        .unwrap()
        .entry((m, prefix))
        .or_default()
        .push(auth);

    match spec {
        ResponseSpec::Stall => {
            while !shutdown.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        ResponseSpec::Json { status, body } => {
            if write_response_head(&mut stream, status, "application/json", body.len()).is_err() {
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
                        if shutdown.load(Ordering::SeqCst) {
                            return;
                        }
                        std::thread::sleep(Duration::from_secs(1));
                    }
                }
                let _ = stream.write_all(&bytes[stall_at..send]);
            }
        }
    }
}

/// Read one HTTP request: request line, headers, and drain any body per
/// Content-Length. Returns (method, target-with-query,
/// Authorization header value).
fn read_request(stream: &mut TcpStream) -> Option<(String, String, Option<String>)> {
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
    let mut auth = None;
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            let name = name.trim().to_ascii_lowercase();
            let value = value.trim();
            if name == "content-length" {
                content_length = value.parse().unwrap_or(0);
            } else if name == "authorization" {
                auth = Some(value.to_string());
            }
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

    Some((method, target, auth))
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
    let head = format!(
        "HTTP/1.1 {status} {}\r\nContent-Type: {content_type}\r\nContent-Length: {content_length}\r\nConnection: close\r\n\r\n",
        reason_of(status)
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
