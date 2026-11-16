//! Shared helpers for integration tests that spawn the real `seedling` binary as
//! a subprocess (as opposed to `tests/static_files.rs`, which exercises
//! `apply_ui_fallback` in-process against a bare `axum::Router`).

use std::{
    net::{SocketAddr, TcpListener, TcpStream},
    path::PathBuf,
    time::{Duration, Instant},
};

/// Fixed, valid-looking 64-hex-char (32 byte) master key — deterministic so
/// tests don't need a `rand`/`hex` dev-dependency just for fixtures.
pub const TEST_MASTER_KEY: &str =
    "0f1e2d3c4b5a69788796a5b4c3d2e1f00f1e2d3c4b5a69788796a5b4c3d2e1f0";

pub fn seedling_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_seedling"))
}

/// Binds an ephemeral port and immediately releases it. Small TOCTOU race
/// (something else could grab it before `seedling` does), acceptable for tests.
pub fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    listener.local_addr().expect("local_addr").port()
}

/// Runs `seedling <args…>` on the database at `db_path` and returns whether
/// it succeeded, with what it printed on standard output and on standard
/// error.
// Each test crate includes this module and uses part of it.
#[allow(dead_code)]
pub fn seedling(db_path: &std::path::Path, args: &[&str]) -> (bool, String, String) {
    let out = std::process::Command::new(seedling_bin())
        .args(args)
        .env("BRIGID_MASTER_KEY", TEST_MASTER_KEY)
        .env("BRIGID_SERVER__DOMAIN", "localhost")
        .env("BRIGID_DATABASE__PATH", db_path.display().to_string())
        .output()
        .expect("run seedling");
    (
        out.status.success(),
        String::from_utf8(out.stdout).expect("UTF-8 output"),
        String::from_utf8(out.stderr).expect("UTF-8 output"),
    )
}

/// Opens sign-up on the database at `db_path`, creating it if needed: what
/// the tests that create accounts start from. A server that nobody opened
/// takes accounts by link only, which has tests of its own.
// Each test crate includes this module and uses part of it.
#[allow(dead_code)]
pub fn open_sign_up(db_path: &std::path::Path) {
    let (ok, out, err) = seedling(db_path, &["signup", "open"]);
    assert!(
        ok && out.trim() == "open",
        "seedling signup open: {out}{err}"
    );
}

pub fn wait_until_listening(addr: SocketAddr, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if TcpStream::connect(addr).is_ok() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}
