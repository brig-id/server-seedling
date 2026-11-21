//! `seedling init` on a fresh machine, as its operator types it: what it
//! writes is enough for the server to start and print the setup link of the
//! first administrator. And the systemd unit it writes is one systemd takes.

mod common;

use std::{
    fs,
    io::{BufRead, BufReader},
    net::SocketAddr,
    process::{Command, Stdio},
    time::Duration,
};

use common::{free_port, seedling_bin, wait_until_listening};

#[test]
fn a_fresh_machine_runs_from_what_init_writes_and_prints_the_setup_link() {
    // The port is free when chosen, not reserved: another test running at the
    // same time can take it before the server binds it. Then the server stops,
    // and the whole machine is made again on another port.
    let mut tries = 0;
    loop {
        tries += 1;
        match fresh_machine() {
            Ok(()) => return,
            Err(lost) if tries < 3 => eprintln!("port taken meanwhile, trying another: {lost}"),
            Err(lost) => panic!("the server never listened: {lost}"),
        }
    }
}

/// One fresh machine. `Err` only when the server stopped without listening
/// (its port was taken); every other failure panics.
fn fresh_machine() -> Result<(), String> {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("etc/seedling.toml");
    let key = dir.path().join("etc/master.key");
    let data = dir.path().join("data");
    let port = free_port();

    let init = |extra: &[&str]| {
        Command::new(seedling_bin())
            .arg("--config")
            .arg(&config)
            .args(["init", "--domain", "id.example.org", "--port"])
            .arg(port.to_string())
            .arg("--key-file")
            .arg(&key)
            .arg("--data-dir")
            .arg(&data)
            .args(extra)
            // Nothing of the environment: what init wrote is all there is.
            .env_clear()
            .output()
            .expect("run seedling init")
    };
    let out = init(&[]);
    let said = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{said}");
    assert!(said.contains("Keep a copy of"), "{said}");
    let written_key = fs::read_to_string(&key).unwrap();

    // Twice: refused, and nothing changes.
    let again = init(&[]);
    assert!(!again.status.success());
    assert!(
        String::from_utf8_lossy(&again.stderr).contains("overwrites nothing"),
        "{}",
        String::from_utf8_lossy(&again.stderr)
    );
    assert_eq!(fs::read_to_string(&key).unwrap(), written_key);

    // The server, from that configuration and that key only.
    let mut server = Command::new(seedling_bin())
        .arg("--config")
        .arg(&config)
        .env_clear()
        .env("BRIGID_MASTER_KEY_FILE", &key)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start seedling");
    let stderr = server.stderr.take().unwrap();
    let link = BufReader::new(stderr)
        .lines()
        .map_while(Result::ok)
        .find_map(|line| {
            line.split_whitespace()
                .find(|w| w.contains("/setup/#link="))
                .map(str::to_owned)
        });
    // Instrumented for coverage, the server starts slowly: a generous wait.
    let listening = wait_until_listening(
        SocketAddr::from(([127, 0, 0, 1], port)),
        Duration::from_secs(60),
    );
    let stopped = server.try_wait().ok().flatten();
    let _ = server.kill();
    let _ = server.wait();
    if !listening && let Some(status) = stopped {
        return Err(format!("the server on port {port} ended ({status})"));
    }

    let link = link.expect("the server prints its setup link");
    assert!(
        link.starts_with("https://id.example.org/setup/#link="),
        "{link}"
    );
    assert!(listening, "the server listens on the port init wrote");
    assert!(
        data.join("brigid.db").exists(),
        "the database is in the data directory"
    );
    Ok(())
}

#[test]
fn init_says_when_its_port_is_taken() {
    let dir = tempfile::tempdir().unwrap();
    let taken = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = taken.local_addr().unwrap().port();
    let out = Command::new(seedling_bin())
        .arg("--config")
        .arg(dir.path().join("seedling.toml"))
        .args(["init", "--domain", "id.example.org", "--port"])
        .arg(port.to_string())
        .arg("--key-file")
        .arg(dir.path().join("master.key"))
        .arg("--data-dir")
        .arg(dir.path().join("data"))
        .env_clear()
        .output()
        .unwrap();
    let said = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{said}");
    assert!(
        said.contains(&format!("already listens on port {port}")),
        "{said}"
    );
}

#[test]
fn a_bad_domain_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("seedling.toml");
    let out = Command::new(seedling_bin())
        .arg("--config")
        .arg(&config)
        .args(["init", "--domain", "https://id.example.org/"])
        .arg("--key-file")
        .arg(dir.path().join("master.key"))
        .arg("--data-dir")
        .arg(dir.path().join("data"))
        .env_clear()
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("not a domain name"));
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
}

/// The unit is checked by systemd itself where it is installed (the CI
/// runners have it). Every directive must be one systemd knows.
#[test]
fn systemd_accepts_the_unit() {
    let Ok(probe) = Command::new("systemd-analyze").arg("--version").output() else {
        eprintln!("no systemd-analyze here: skipped");
        return;
    };
    if !probe.status.success() {
        eprintln!("systemd-analyze does not run here: skipped");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let unit = dir.path().join("seedling.service");
    fs::write(
        &unit,
        seedling::machine::unit_text(&seedling_bin(), &dir.path().join("master.key")),
    )
    .unwrap();
    let out = Command::new("systemd-analyze")
        .arg("verify")
        .arg(&unit)
        .output()
        .unwrap();
    let said = String::from_utf8_lossy(&out.stderr);
    // A machine's systemd also speaks of its own units (an option of theirs
    // it no longer supports): only what it says of this one counts.
    let ours: Vec<&str> = said
        .lines()
        .filter(|line| line.contains("seedling.service"))
        .collect();
    assert!(out.status.success() && ours.is_empty(), "{said}");
}
