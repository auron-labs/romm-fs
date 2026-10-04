//! Tiny local HTTP fixture server for tests — implements the documented
//! RomM contract subset with scripted failures and request counting.
//!
//! Used to prove behavior against the REAL client: "no download on listing",
//! "one download for concurrent reads", failure/retry paths — by counting
//! actual requests, not mocking internals.

use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// How the fixture answers one route.
#[derive(Clone, Debug)]
pub enum ResponseSpec {
    Json { status: u16, body: String },
    /// Serve `bytes`; may truncate to `truncate_at` or stall after
    /// `stall_after_bytes` without sending more (for stall-timeout tests).
    Bytes {
        status: u16,
        bytes: Vec<u8>,
        truncate_at: Option<usize>,
        stall_after_bytes: Option<usize>,
    },
}

/// Per-test scripted server. Drop/join stops it.
pub struct FixtureServer {
    base_url: String,
    /// (method,path-prefix) -> requests seen — proofs for "did not download".
    counts: Arc<Mutex<std::collections::HashMap<(String, String), AtomicUsize>>>,
    /// (method, path prefix) -> response spec. First prefix match wins.
    routes: Arc<Mutex<Vec<(String, String, ResponseSpec)>>>,
    shutdown: Arc<std::sync::atomic::AtomicBool>,
    _accept: Option<std::thread::JoinHandle<()>>,
}

impl FixtureServer {
    /// Start on an ephemeral localhost port.
    pub fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("fixture bind");
        todo!("accept loop: thread-per-conn; read request line+headers; match (method, path prefix) in routes; count; write spec response (honor truncate/stall); Connection: close")
    }

    pub fn url(&self) -> &str {
        &self.base_url
    }

    /// Register/override a route: `on("GET", "/api/roms", spec)`.
    pub fn on(&self, method: &str, path_prefix: &str, spec: ResponseSpec) {
        let _ = (method, path_prefix, spec);
        todo!()
    }

    /// Requests seen for (method, path prefix).
    pub fn count(&self, method: &str, path_prefix: &str) -> usize {
        let _ = (method, path_prefix);
        todo!()
    }
}

impl Drop for FixtureServer {
    fn drop(&mut self) {
        todo!("set shutdown, wake accept, join")
    }
}

/// Builder for the catalogue payloads matching `.planning/API-CONTRACT.md`.
pub mod contract {
    /// `TokenResponse` JSON for POST /api/token.
    pub fn token_ok() -> String {
        todo!()
    }
    /// Platform list JSON.
    pub fn platforms(items: &[(i64, &str, &str, &str)]) -> String {
        let _ = items;
        todo!()
    }
    /// One `items` page of GET /api/roms.
    pub fn roms_page(items: &[serde_json::Value], total: usize, limit: usize, offset: usize) -> String {
        let _ = (items, total, limit, offset);
        todo!()
    }
    /// A SimpleRomSchema-shaped JSON object for a single-file ROM.
    pub fn rom(id: i64, platform_fs_slug: &str, file_name: &str, size: u64, sha1: &str) -> serde_json::Value {
        let _ = (id, platform_fs_slug, file_name, size, sha1);
        todo!()
    }
    /// A multi-file (unsupported) ROM shape.
    pub fn rom_multi(id: i64, platform_fs_slug: &str, names: &[&str], sizes: &[u64]) -> serde_json::Value {
        let _ = (id, platform_fs_slug, names, sizes);
        todo!()
    }
}
