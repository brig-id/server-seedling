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
//!   it: no file of the database ever belongs to root (brigid-server).

use std::{
    fmt::Write as _,
    fs,
    io::{self, Read as _, Write as _},
    os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _},
    path::{Path, PathBuf},
};

/// Where `seedling init` writes the configuration, and where the server
/// looks for one when `--config` is not given.
pub const CONFIG_PATH: &str = "/etc/brigid/seedling.toml";
/// Where `seedling init` writes the master key, and where the server looks
/// for it when the environment names none.
pub const KEY_PATH: &str = "/etc/brigid/master.key";
/// Where `seedling init --seal` and `seedling seal-key` put the master key
/// sealed by `systemd-creds` (with the machine's TPM when it has one).
pub const SEALED_KEY_PATH: &str = "/etc/brigid/master.key.cred";
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
    /// Seal the new key instead of writing it in clear: with this program
    /// (`systemd-creds`), at this path. Needs `systemd` and `create_key`.
    pub seal: Option<Sealer>,
}

/// How a key is sealed: the program (`systemd-creds`, or a stand-in in
/// tests) and the file it writes.
#[derive(Debug, Clone)]
pub struct Sealer {
    pub program: PathBuf,
    pub path: PathBuf,
}

/// What `init` did.
#[derive(Debug)]
pub struct Initialized {
    pub written: Vec<PathBuf>,
    /// The new key when it was sealed: shown this once, to be kept offline.
    pub sealed_key: Option<String>,
}

#[derive(Debug)]
pub enum InitError {
    /// Not a domain name.
    Domain(String),
    /// A file `init` would write is there already: nothing was written.
    Exists(PathBuf),
    /// `--systemd` with a data directory systemd does not create.
    DataDir(PathBuf),
    /// `--seal` without `--systemd`, or with a key supplied from elsewhere.
    SealNeedsSystemd,
    /// `systemd-creds` failed to seal the key.
    Seal(String),
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
            Self::SealNeedsSystemd => write!(
                f,
                "--seal needs --systemd (systemd hands the sealed key to the service) and a key \
                 made here (not --no-key-file)"
            ),
            Self::Seal(why) => write!(f, "the key could not be sealed: {why}"),
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

/// The line of the unit that hands the key to the service.
const CLEAR_KEY_LINE: &str = "LoadCredential=master.key:";

/// The systemd unit for this executable and this key. `sealed`: the key
/// file is sealed by `systemd-creds`, and systemd unseals it for the service.
#[must_use]
pub fn unit_text(executable: &Path, key_file: &Path, sealed: bool) -> String {
    let directive = if sealed {
        "LoadCredentialEncrypted"
    } else {
        "LoadCredential"
    };
    UNIT.replace(
        "ExecStart=/usr/local/bin/seedling",
        &format!("ExecStart={}", executable.display()),
    )
    .replace(
        "LoadCredential=master.key:/etc/brigid/master.key",
        &format!(
            "{directive}={}:{}",
            brigid_server::SEALED_KEY_NAME,
            key_file.display()
        ),
    )
}

/// `unit` handing the service the sealed key at `sealed` instead of a clear
/// key file. `None` when the unit has no clear key line to replace.
#[must_use]
pub fn sealed_unit(unit: &str, sealed: &Path) -> Option<String> {
    let mut found = false;
    let lines: Vec<String> = unit
        .lines()
        .map(|line| {
            if line.starts_with(CLEAR_KEY_LINE) {
                found = true;
                format!(
                    "LoadCredentialEncrypted={}:{}",
                    brigid_server::SEALED_KEY_NAME,
                    sealed.display()
                )
            } else {
                line.to_owned()
            }
        })
        .collect();
    found.then(|| lines.join("\n") + "\n")
}

/// Whether `key` is a master key: 64 hex characters, spaces around aside.
#[must_use]
pub fn valid_key(key: &str) -> bool {
    let key = key.trim();
    key.len() == 64 && key.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Seals `key_hex` at `path` with `program` (`systemd-creds encrypt`, the TPM
/// when the machine has one, else the host key). Refuses a file that exists.
pub fn seal_key(program: &Path, key_hex: &str, path: &Path) -> Result<(), InitError> {
    use std::process::{Command, Stdio};
    if path.exists() {
        return Err(InitError::Exists(path.to_owned()));
    }
    if let Some(dir) = path.parent()
        && !dir.as_os_str().is_empty()
    {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o755)
            .create(dir)
            .map_err(|e| InitError::Io(dir.to_owned(), e))?;
    }
    let mut child = Command::new(program)
        .arg("encrypt")
        .arg(format!("--name={}", brigid_server::SEALED_KEY_NAME))
        .arg("--with-key=auto")
        .arg("-")
        .arg(path)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| InitError::Seal(format!("cannot run {}: {e}", program.display())))?;
    // A program that fails at once (no TPM) ends before reading the key: the
    // write then breaks, and what it said on stderr is the reason to give.
    let written = child
        .stdin
        .take()
        .map(|mut stdin| stdin.write_all(key_hex.trim().as_bytes()));
    let out = child
        .wait_with_output()
        .map_err(|e| InitError::Seal(e.to_string()))?;
    let said = String::from_utf8_lossy(&out.stderr).trim().to_owned();
    if !out.status.success() || !path.exists() {
        let _ = fs::remove_file(path);
        return Err(InitError::Seal(if said.is_empty() {
            format!("{} ended with {}", program.display(), out.status)
        } else {
            said
        }));
    }
    if let Some(Err(e)) = written {
        let _ = fs::remove_file(path);
        return Err(InitError::Seal(format!(
            "the key could not be handed over: {e}"
        )));
    }
    Ok(())
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
pub fn init(options: &InitOptions) -> Result<Initialized, InitError> {
    if !valid_domain(&options.domain) {
        return Err(InitError::Domain(options.domain.clone()));
    }
    if options.seal.is_some() && (options.systemd.is_none() || !options.create_key) {
        return Err(InitError::SealNeedsSystemd);
    }
    if options.systemd.is_some() && options.data_dir != Path::new(DATA_DIR) {
        return Err(InitError::DataDir(options.data_dir.clone()));
    }

    let mut files: Vec<(PathBuf, Vec<u8>, u32)> = Vec::new();
    let mut sealed_key = None;
    if options.create_key {
        let key = new_key_hex().map_err(|e| InitError::Io("/dev/urandom".into(), e))?;
        if options.seal.is_some() {
            sealed_key = Some(key.trim().to_owned());
        } else {
            files.push((options.key_file.clone(), key.into_bytes(), 0o600));
        }
    }
    // The unit hands the service the sealed file, or the clear one.
    let unit_key = options
        .seal
        .as_ref()
        .map_or(options.key_file.as_path(), |s| s.path.as_path());
    files.push((
        options.config.clone(),
        config_text(options).into_bytes(),
        0o644,
    ));
    if let Some(executable) = &options.systemd {
        files.push((
            UNIT_PATH.into(),
            unit_text(executable, unit_key, options.seal.is_some()).into_bytes(),
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
    // Sealed first: if systemd-creds cannot, nothing else is written.
    if let (Some(sealer), Some(key)) = (&options.seal, &sealed_key) {
        seal_key(&sealer.program, key, &sealer.path)?;
        written.push(sealer.path.clone());
    }
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
    Ok(Initialized {
        written,
        sealed_key,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::MetadataExt as _;

    fn options(dir: &Path) -> InitOptions {
        InitOptions {
            domain: "id.example.org".into(),
            config: dir.join("etc/seedling.toml"),
            key_file: dir.join("etc/master.key"),
            create_key: true,
            data_dir: dir.join("data"),
            port: 8080,
            systemd: None,
            seal: None,
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
        let done = init(&o).unwrap();
        assert_eq!(done.written, vec![o.key_file.clone(), o.config.clone()]);
        assert!(done.sealed_key.is_none());

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
        assert_eq!(init(&o).unwrap().written, vec![o.config.clone()]);
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
            false,
        );
        assert!(unit.contains("\nExecStart=/opt/seedling/bin/seedling\n"));
        assert!(unit.contains("\nLoadCredential=master.key:/mnt/keys/k\n"));
        assert!(unit.contains("\nUser=brigid\n"));
        assert!(!unit.contains("/usr/local/bin/seedling"));
    }

    /// A stand-in for `systemd-creds encrypt`: writes what it reads to the
    /// output file, prefixed, when the arguments are the expected ones.
    fn fake_sealer(dir: &Path) -> PathBuf {
        use std::os::unix::fs::PermissionsExt as _;
        let program = dir.join("systemd-creds");
        fs::write(
            &program,
            "#!/bin/sh\n[ \"$1\" = encrypt ] && [ \"$2\" = --name=master.key ] \\\n  \
             && [ \"$3\" = --with-key=auto ] && [ \"$4\" = - ] \\\n  \
             && { printf sealed: > \"$5\"; cat >> \"$5\"; exit 0; }\necho no TPM >&2; exit 1\n",
        )
        .unwrap();
        fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
        program
    }

    #[test]
    fn a_key_is_sealed_by_systemd_creds_and_never_over_another() {
        let dir = tempfile::tempdir().unwrap();
        let program = fake_sealer(dir.path());
        let cred = dir.path().join("etc/master.key.cred");
        let key = "ab".repeat(32);
        seal_key(&program, &format!("{key}\n"), &cred).unwrap();
        assert_eq!(fs::read_to_string(&cred).unwrap(), format!("sealed:{key}"));
        assert!(matches!(
            seal_key(&program, &key, &cred),
            Err(InitError::Exists(p)) if p == cred
        ));
        // A failure leaves no file behind.
        let failing = dir.path().join("false");
        fs::write(&failing, "#!/bin/sh\necho no TPM >&2\nexit 1\n").unwrap();
        {
            use std::os::unix::fs::PermissionsExt as _;
            fs::set_permissions(&failing, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let other = dir.path().join("other.cred");
        assert!(matches!(
            seal_key(&failing, &key, &other),
            Err(InitError::Seal(why)) if why.contains("no TPM")
        ));
        assert!(!other.exists());
    }

    #[test]
    fn sealing_needs_systemd_and_a_key_made_here() {
        let dir = tempfile::tempdir().unwrap();
        let o = InitOptions {
            seal: Some(Sealer {
                program: fake_sealer(dir.path()),
                path: dir.path().join("master.key.cred"),
            }),
            ..options(dir.path())
        };
        assert!(matches!(init(&o), Err(InitError::SealNeedsSystemd)));
        assert!(!o.config.exists());
    }

    #[test]
    fn the_unit_hands_the_service_a_sealed_key() {
        let unit = unit_text(
            Path::new("/usr/local/bin/seedling"),
            Path::new(SEALED_KEY_PATH),
            true,
        );
        assert!(
            unit.contains("\nLoadCredentialEncrypted=master.key:/etc/brigid/master.key.cred\n")
        );
        assert!(!unit.contains("\nLoadCredential="));

        // An installed unit moves from the clear key to the sealed one.
        let clear = unit_text(
            Path::new("/usr/local/bin/seedling"),
            Path::new(KEY_PATH),
            false,
        );
        let moved = sealed_unit(&clear, Path::new(SEALED_KEY_PATH)).unwrap();
        assert_eq!(moved, unit);
        assert!(sealed_unit(&moved, Path::new(SEALED_KEY_PATH)).is_none());
    }

    #[test]
    fn a_key_is_64_hex_characters() {
        assert!(valid_key(&format!(" {}\n", "0f".repeat(32))));
        assert!(!valid_key(&"0f".repeat(31)));
        assert!(!valid_key(&"zz".repeat(32)));
    }
}
