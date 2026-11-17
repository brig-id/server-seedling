//! The server on a machine of its own: `seedling init`, where things live,
//! and who the process runs as.
//!
//! - `seedling init` writes a minimal configuration and a master key file,
//!   and with `--systemd` the unit and the system user that run it. It never
//!   overwrites a file: a key replaced is a database lost.
//! - The server never runs as root. Under systemd it runs as `brigid`, and
//!   the key, a root-only file, is handed to the service alone.
//! - A command typed as root on the machine (`seedling client list`) reads
//!   the key as root, then becomes the owner of the database before it opens
//!   it: no file of the database ever belongs to root.

use std::{
    fmt::Write as _,
    fs,
    io::{self, Read as _, Write as _},
    os::unix::fs::{DirBuilderExt as _, MetadataExt as _, OpenOptionsExt as _},
    path::{Path, PathBuf},
};

/// Where `seedling init` writes the configuration, and where the server
/// looks for one when `--config` is not given.
pub const CONFIG_PATH: &str = "/etc/brigid/seedling.toml";
/// Where `seedling init` writes the master key, and where the server looks
/// for it when the environment names none.
pub const KEY_PATH: &str = "/etc/brigid/master.key";
/// The data directory: systemd creates it for the service (`StateDirectory`).
pub const DATA_DIR: &str = "/var/lib/brigid";
/// The unit, and the system user, written by `seedling init --systemd`.
pub const UNIT_PATH: &str = "/etc/systemd/system/seedling.service";
pub const SYSUSERS_PATH: &str = "/etc/sysusers.d/brigid.conf";

/// What to type after `seedling init --systemd`: printed by it, and shown by
/// step 3 of docs/install.md, which a test keeps identical.
pub const START_COMMANDS: &str = "\
sudo systemd-sysusers
sudo systemctl daemon-reload
sudo systemctl enable --now seedling
sudo journalctl -u seedling -n 20 --no-pager
";

/// Whether something on this machine already listens on `port`, where the
/// server would.
#[must_use]
pub fn port_in_use(port: u16) -> bool {
    std::net::TcpListener::bind(("127.0.0.1", port)).is_err()
}

const UNIT: &str = include_str!("../deploy/systemd/seedling.service");
const SYSUSERS: &str = include_str!("../deploy/systemd/brigid.conf");

/// The effective user of this process.
#[must_use]
pub fn effective_uid() -> u32 {
    // SAFETY: geteuid has no preconditions and cannot fail.
    unsafe { libc::geteuid() }
}

/// What `seedling init` is asked to write.
#[derive(Debug, Clone)]
pub struct InitOptions {
    /// The domain people reach the server at: `id.example.org`.
    pub domain: String,
    /// The configuration file to write.
    pub config: PathBuf,
    /// Where the master key lives.
    pub key_file: PathBuf,
    /// Whether to create the key there (no: it is supplied from elsewhere,
    /// another disk for instance, at that path).
    pub create_key: bool,
    /// The directory of the database.
    pub data_dir: PathBuf,
    /// The local port the server listens on, behind a reverse proxy.
    pub port: u16,
    /// Also the systemd unit and the system user, with this executable.
    pub systemd: Option<PathBuf>,
}

#[derive(Debug)]
pub enum InitError {
    /// Not a domain name.
    Domain(String),
    /// A file `init` would write is there already: nothing was written.
    Exists(PathBuf),
    /// `--systemd` with a data directory systemd does not create.
    DataDir(PathBuf),
    Io(PathBuf, io::Error),
}

impl std::fmt::Display for InitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Domain(d) => write!(
                f,
                "`{d}` is not a domain name: give the name people reach this server at, \
                 such as id.example.org (no scheme, no path, no port)"
            ),
            Self::Exists(p) => write!(
                f,
                "{} exists already, and seedling init overwrites nothing (a master key \
                 replaced is a database lost). Nothing was written.",
                p.display()
            ),
            Self::DataDir(p) => write!(
                f,
                "with --systemd the data directory is {DATA_DIR}, which systemd creates for \
                 the service; {} is not",
                p.display()
            ),
            Self::Io(p, e) => write!(f, "{}: {e}", p.display()),
        }
    }
}

impl std::error::Error for InitError {}

/// Whether `domain` is a host name a server can be reached at.
#[must_use]
pub fn valid_domain(domain: &str) -> bool {
    !domain.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && url::Url::parse(&format!("https://{domain}/"))
            .ok()
            .is_some_and(|u| {
                u.host_str() == Some(domain)
                    && u.port().is_none()
                    && u.path() == "/"
                    && u.username().is_empty()
            })
}

/// The configuration `seedling init` writes: what a server behind a reverse
/// proxy needs, and nothing that has a good default.
#[must_use]
pub fn config_text(o: &InitOptions) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "# brig·id seedling, written by `seedling init`.");
    let _ = writeln!(
        s,
        "# Every setting and its default: seedling.example.toml in brig-id/server-seedling."
    );
    let _ = writeln!(
        s,
        "# Any of them can also come from the environment (BRIGID_SERVER__PORT=8443, …)."
    );
    let _ = writeln!(
        s,
        "# The master key is never here: it is {}.",
        o.key_file.display()
    );
    s.push('\n');
    let _ = writeln!(s, "[server]");
    let _ = writeln!(s, "domain = {:?}", o.domain);
    let _ = writeln!(s, "public_url = {:?}", format!("https://{}", o.domain));
    let _ = writeln!(
        s,
        "# Behind a reverse proxy that holds the certificate (Caddy, nginx): the"
    );
    let _ = writeln!(s, "# server listens on this machine only.");
    let _ = writeln!(s, "host = \"127.0.0.1\"");
    let _ = writeln!(s, "port = {}", o.port);
    let _ = writeln!(s, "# Without a proxy, with a certificate of your own:");
    let _ = writeln!(s, "# host = \"0.0.0.0\"");
    let _ = writeln!(s, "# port = 443");
    let _ = writeln!(s, "# tls_cert = \"/etc/brigid/cert.pem\"");
    let _ = writeln!(s, "# tls_key = \"/etc/brigid/key.pem\"");
    s.push('\n');
    let _ = writeln!(s, "[database]");
    let _ = writeln!(
        s,
        "path = {:?}",
        o.data_dir.join("brigid.db").display().to_string()
    );
    s.push('\n');
    let _ = writeln!(s, "[security]");
    let _ = writeln!(
        s,
        "# Only the proxy reaches the server, and it sets X-Forwarded-For: rate limits"
    );
    let _ = writeln!(s, "# then count each client, not the proxy.");
    let _ = writeln!(s, "trust_forwarded_for = true");
    s
}

/// The systemd unit for this executable and this key.
#[must_use]
pub fn unit_text(executable: &Path, key_file: &Path) -> String {
    UNIT.replace(
        "ExecStart=/usr/local/bin/seedling",
        &format!("ExecStart={}", executable.display()),
    )
    .replace(
        "LoadCredential=master.key:/etc/brigid/master.key",
        &format!("LoadCredential=master.key:{}", key_file.display()),
    )
}

/// The system user, for systemd-sysusers.
#[must_use]
pub fn sysusers_text() -> &'static str {
    SYSUSERS
}

/// 32 random bytes from the kernel, in hex: a new master key.
fn new_key_hex() -> io::Result<String> {
    let mut bytes = [0u8; 32];
    fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    let mut hex = String::with_capacity(65);
    for b in bytes {
        let _ = write!(hex, "{b:02x}");
    }
    hex.push('\n');
    Ok(hex)
}

/// Writes `content` to a new file with `mode`, creating its directory.
/// Refuses a file that exists.
fn write_new(path: &Path, content: &[u8], mode: u32) -> Result<(), InitError> {
    if let Some(dir) = path.parent()
        && !dir.as_os_str().is_empty()
    {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o755)
            .create(dir)
            .map_err(|e| InitError::Io(dir.to_owned(), e))?;
    }
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(path)
        .map_err(|e| match e.kind() {
            io::ErrorKind::AlreadyExists => InitError::Exists(path.to_owned()),
            _ => InitError::Io(path.to_owned(), e),
        })?;
    file.write_all(content)
        .and_then(|()| file.sync_all())
        .map_err(|e| InitError::Io(path.to_owned(), e))
}

/// Writes what `options` asks for, or nothing: every file is checked before
/// the first is written. Returns the files written.
pub fn init(options: &InitOptions) -> Result<Vec<PathBuf>, InitError> {
    if !valid_domain(&options.domain) {
        return Err(InitError::Domain(options.domain.clone()));
    }
    if options.systemd.is_some() && options.data_dir != Path::new(DATA_DIR) {
        return Err(InitError::DataDir(options.data_dir.clone()));
    }

    let mut files: Vec<(PathBuf, Vec<u8>, u32)> = Vec::new();
    if options.create_key {
        let key = new_key_hex().map_err(|e| InitError::Io("/dev/urandom".into(), e))?;
        files.push((options.key_file.clone(), key.into_bytes(), 0o600));
    }
    files.push((
        options.config.clone(),
        config_text(options).into_bytes(),
        0o644,
    ));
    if let Some(executable) = &options.systemd {
        files.push((
            UNIT_PATH.into(),
            unit_text(executable, &options.key_file).into_bytes(),
            0o644,
        ));
        files.push((
            SYSUSERS_PATH.into(),
            sysusers_text().as_bytes().to_vec(),
            0o644,
        ));
    }
    if let Some((path, _, _)) = files.iter().find(|(path, _, _)| path.exists()) {
        return Err(InitError::Exists(path.clone()));
    }

    let mut written = Vec::new();
    for (path, content, mode) in &files {
        if let Err(e) = write_new(path, content, *mode) {
            // Nothing half-done: what this run wrote goes.
            for done in &written {
                let _ = fs::remove_file(done);
            }
            return Err(e);
        }
        written.push(path.clone());
    }

    // Without systemd, the data directory is the operator's to own: it is
    // made here, for the user who runs this. With it, systemd makes it.
    if options.systemd.is_none() {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&options.data_dir)
            .map_err(|e| InitError::Io(options.data_dir.clone(), e))?;
    }
    Ok(written)
}

/// Becomes the owner of the database's directory, for a command run as
/// root: the files of the database never belong to root, or the server,
/// which does not run as root, could not open them.
pub fn become_owner_of_database(database: &Path) -> Result<(), String> {
    let dir = match database.parent() {
        Some(d) if !d.as_os_str().is_empty() => d.to_owned(),
        _ => PathBuf::from("."),
    };
    let meta = fs::metadata(&dir).map_err(|e| {
        format!(
            "{}: {e}. Start the server once (systemctl start seedling): it creates its data \
             directory, owned by the user it runs as.",
            dir.display()
        )
    })?;
    let (uid, gid) = (meta.uid(), meta.gid());
    if uid == 0 {
        return Err(format!(
            "{} belongs to root, and seedling does not run as root. Give it to the user the \
             server runs as (with systemd: brigid).",
            dir.display()
        ));
    }
    // SAFETY: plain system calls with no pointer but the empty group list;
    // glibc and musl apply them to every thread of the process.
    let ok = unsafe {
        libc::setgroups(0, std::ptr::null()) == 0
            && libc::setgid(gid) == 0
            && libc::setuid(uid) == 0
    };
    if !ok || effective_uid() != uid {
        return Err(format!(
            "could not become the owner of {} (uid {uid}): {}",
            dir.display(),
            io::Error::last_os_error()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(dir: &Path) -> InitOptions {
        InitOptions {
            domain: "id.example.org".into(),
            config: dir.join("etc/seedling.toml"),
            key_file: dir.join("etc/master.key"),
            create_key: true,
            data_dir: dir.join("data"),
            port: 8080,
            systemd: None,
        }
    }

    #[test]
    fn a_domain_is_a_host_name_and_nothing_more() {
        for good in ["id.example.org", "helm.brig.id", "xn--bcher-kva.example"] {
            assert!(valid_domain(good), "{good}");
        }
        for bad in [
            "",
            "localhost",
            "https://id.example.org",
            "id.example.org/",
            "id.example.org:443",
            "user@id.example.org",
            ".example.org",
            "id example.org",
            "ID.example.org",
        ] {
            assert!(!valid_domain(bad), "{bad}");
        }
    }

    #[test]
    fn init_writes_a_key_and_a_configuration_and_never_overwrites_them() {
        let dir = tempfile::tempdir().unwrap();
        let o = options(dir.path());
        let written = init(&o).unwrap();
        assert_eq!(written, vec![o.key_file.clone(), o.config.clone()]);

        let key = fs::read_to_string(&o.key_file).unwrap();
        assert_eq!(key.trim().len(), 64);
        assert!(key.trim().chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(fs::metadata(&o.key_file).unwrap().mode() & 0o777, 0o600);
        assert_eq!(fs::metadata(&o.data_dir).unwrap().mode() & 0o777, 0o700);
        let config = fs::read_to_string(&o.config).unwrap();
        assert!(config.contains("domain = \"id.example.org\""), "{config}");
        assert!(
            !config.contains(key.trim()),
            "the key is never in the configuration"
        );

        // Again: refused, and the key is the same.
        assert!(matches!(init(&o), Err(InitError::Exists(p)) if p == o.key_file));
        assert_eq!(fs::read_to_string(&o.key_file).unwrap(), key);
        // A second server gets another key.
        let other = tempfile::tempdir().unwrap();
        init(&options(other.path())).unwrap();
        assert_ne!(
            fs::read_to_string(options(other.path()).key_file).unwrap(),
            key
        );
    }

    #[test]
    fn a_file_in_the_way_stops_everything_before_anything_is_written() {
        let dir = tempfile::tempdir().unwrap();
        let o = options(dir.path());
        fs::create_dir_all(o.config.parent().unwrap()).unwrap();
        fs::write(&o.config, "mine").unwrap();
        assert!(matches!(init(&o), Err(InitError::Exists(p)) if p == o.config));
        assert!(
            !o.key_file.exists(),
            "no key written when the configuration is refused"
        );
        assert_eq!(fs::read_to_string(&o.config).unwrap(), "mine");
    }

    #[test]
    fn a_key_supplied_from_elsewhere_is_not_created() {
        let dir = tempfile::tempdir().unwrap();
        let o = InitOptions {
            create_key: false,
            ..options(dir.path())
        };
        assert_eq!(init(&o).unwrap(), vec![o.config.clone()]);
        assert!(!o.key_file.exists());
    }

    #[test]
    fn with_systemd_the_data_directory_is_the_one_systemd_creates() {
        let dir = tempfile::tempdir().unwrap();
        let o = InitOptions {
            systemd: Some("/usr/local/bin/seedling".into()),
            ..options(dir.path())
        };
        assert!(matches!(init(&o), Err(InitError::DataDir(_))));
        assert!(!o.key_file.exists());
    }

    #[test]
    fn the_unit_runs_this_executable_with_this_key() {
        let unit = unit_text(
            Path::new("/opt/seedling/bin/seedling"),
            Path::new("/mnt/keys/k"),
        );
        assert!(unit.contains("\nExecStart=/opt/seedling/bin/seedling\n"));
        assert!(unit.contains("\nLoadCredential=master.key:/mnt/keys/k\n"));
        assert!(unit.contains("\nUser=brigid\n"));
        assert!(!unit.contains("/usr/local/bin/seedling"));
    }
}
