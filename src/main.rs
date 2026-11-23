//! brig·id `seedling`: the server on a machine of its own.
//!
//! The server and its commands are `brigid-server` (core), the same for every
//! edition. seedling adds `init`, which prepares the machine.

use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};

use seedling::{EDITION, machine};

#[derive(Parser)]
#[command(name = "seedling", version, about = "brig·id single-server deployment")]
struct Cli {
    /// Path to the TOML configuration file.
    ///
    /// Optional — if omitted, configuration is read from /etc/brigid/seedling.toml
    /// when it exists, and from `BRIGID_*` environment variables (the
    /// recommended setup for container deployments). Accepted on either side
    /// of a subcommand.
    #[arg(long, global = true)]
    config: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Prepare this machine: a configuration and a master key, and with
    /// `--systemd` the unit and the system user that run the server. Writes
    /// to `--config` (default /etc/brigid/seedling.toml). Overwrites nothing.
    Init(InitArgs),

    /// Seal an existing master key with this machine (`systemd-creds`, its
    /// TPM when it has one) at /etc/brigid/master.key.cred, and switch the
    /// unit written by `init --systemd` to it. For a server installed with a
    /// clear key, or moved to a new machine from the offline copy. The clear
    /// file is left for you to delete once the server restarted on the
    /// sealed key.
    SealKey {
        /// The key to seal: a file holding it (64 hex characters).
        #[arg(long, default_value = machine::KEY_PATH)]
        from: PathBuf,
    },

    #[command(flatten)]
    Server(brigid_server::Command),
}

/// What `seedling init` is given.
#[derive(clap::Args)]
struct InitArgs {
    /// The domain people reach this server at: `id.example.org`.
    #[arg(long)]
    domain: String,
    /// Where the master key lives.
    #[arg(long, default_value = machine::KEY_PATH)]
    key_file: PathBuf,
    /// Do not create the key: it is supplied at `--key-file` from
    /// elsewhere (another disk, for one), so that a copy of this disk
    /// cannot be read without it.
    #[arg(long)]
    no_key_file: bool,
    /// The directory of the database.
    #[arg(long, default_value = machine::DATA_DIR)]
    data_dir: PathBuf,
    /// The local port the server listens on, behind a reverse proxy.
    #[arg(long, default_value_t = 8080)]
    port: u16,
    /// Also write the systemd unit and the system user `brigid`.
    #[arg(long)]
    systemd: bool,
    /// Seal the master key with this machine (`systemd-creds`: its TPM
    /// when it has one) instead of writing it in clear. A copy of the
    /// disk is then useless without the machine. The key is printed
    /// once: keep it offline, it is the only way back after the machine
    /// or its TPM is replaced. Needs `--systemd`.
    #[arg(long, requires = "systemd", conflicts_with = "no_key_file")]
    seal: bool,
    /// The server obtains and renews its certificate itself (Let's
    /// Encrypt), alone on port 443, with no proxy in front. The authority
    /// must reach this machine on port 443 under `--domain`.
    #[arg(long, conflicts_with = "port")]
    acme: bool,
    /// An e-mail address the certificate authority may write to (with
    /// `--acme`): it warns there when a renewal keeps failing.
    #[arg(long, requires = "acme")]
    acme_contact: Option<String>,
}

/// Handles `seedling init`.
fn init_command(config: Option<&Path>, args: &InitArgs) {
    let InitArgs {
        domain,
        key_file,
        no_key_file,
        data_dir,
        port,
        systemd,
        seal,
        acme,
        acme_contact,
    } = args;
    let (no_key_file, systemd, seal) = (*no_key_file, *systemd, *seal);
    let listener = if *acme {
        machine::Listener::Acme {
            contact: acme_contact.clone(),
        }
    } else {
        machine::Listener::BehindProxy { port: *port }
    };
    let port = listener.port();
    let executable = systemd.then(|| {
        std::env::current_exe()
            .and_then(std::fs::canonicalize)
            .expect("cannot tell where this executable is")
    });
    let options = machine::InitOptions {
        domain: domain.to_owned(),
        config: config.map_or_else(|| machine::CONFIG_PATH.into(), Path::to_path_buf),
        key_file: key_file.to_owned(),
        create_key: !no_key_file,
        data_dir: data_dir.to_owned(),
        listener,
        systemd: executable,
        seal: seal.then(|| machine::Sealer {
            program: "systemd-creds".into(),
            path: machine::SEALED_KEY_PATH.into(),
        }),
    };
    let done = match machine::init(&options) {
        Ok(done) => done,
        Err(e) => {
            eprintln!("✗ {e}");
            std::process::exit(1);
        }
    };
    for path in &done.written {
        eprintln!("✓ wrote {}", path.display());
    }
    if let Some(key) = &done.sealed_key {
        println!("master_key={key}");
        eprintln!(
            "\nThe master key is sealed by this machine in {}: a copy of the disk alone cannot \
             read the database. The key above is shown this once. Keep it offline: it is the \
             only way back if the machine or its TPM is replaced (`seedling seal-key --from`).",
            machine::SEALED_KEY_PATH
        );
    } else if options.create_key {
        eprintln!(
            "\nKeep a copy of {} somewhere else, offline: without it the database cannot be \
             read, by anyone.",
            options.key_file.display()
        );
    } else {
        eprintln!(
            "\nNo key was created: put it at {} (64 hex characters) before the first start.",
            options.key_file.display()
        );
    }
    match &options.listener {
        machine::Listener::Acme { .. } => eprintln!(
            "\nThe server listens on port 443 and obtains the certificate of {domain} itself, at its \
             first start: the DNS record of {domain} must point to this machine, and port 443 must \
             reach it from the Internet."
        ),
        machine::Listener::BehindProxy { .. } => eprintln!(
            "\nThe server listens on 127.0.0.1:{port}. A reverse proxy holds the certificate of \
             {domain} and forwards to it; or edit {} to let the server obtain one itself \
             (`acme = true`) or give it one of its own.",
            options.config.display()
        ),
    }
    if machine::port_in_use(port) {
        eprintln!(
            "\n⚠ Something on this machine already listens on port {port}: the server could not \
             start there. Change `port` in {}, or run init again elsewhere with --port.",
            options.config.display()
        );
    }
    if systemd {
        eprintln!("\nThen (the last command shows the setup link of the first administrator):\n");
        for line in machine::START_COMMANDS.lines() {
            eprintln!("  {line}");
        }
        eprintln!();
    } else {
        eprintln!(
            "\nThen start `seedling` as the user who owns {}: it prints the setup link of the \
             first administrator.",
            options.data_dir.display()
        );
    }
}

/// Handles `seedling seal-key`.
fn seal_key_command(from: &Path) {
    let key = std::fs::read_to_string(from).unwrap_or_else(|e| {
        eprintln!("✗ {}: {e}", from.display());
        std::process::exit(1);
    });
    if !machine::valid_key(&key) {
        eprintln!(
            "✗ {} does not hold a master key (64 hex characters)",
            from.display()
        );
        std::process::exit(1);
    }
    let sealed = Path::new(machine::SEALED_KEY_PATH);
    if let Err(e) = machine::seal_key(Path::new("systemd-creds"), &key, sealed) {
        eprintln!("✗ {e}");
        std::process::exit(1);
    }
    eprintln!("✓ sealed the key in {}", sealed.display());
    let unit = Path::new(machine::UNIT_PATH);
    match std::fs::read_to_string(unit)
        .ok()
        .and_then(|text| machine::sealed_unit(&text, sealed))
    {
        Some(text) => match std::fs::write(unit, text) {
            Ok(()) => eprintln!("✓ {} now hands the service the sealed key", unit.display()),
            Err(e) => eprintln!(
                "⚠ {}: {e}: change its LoadCredential line yourself",
                unit.display()
            ),
        },
        None => eprintln!(
            "⚠ {} has no LoadCredential=master.key line to change: give the service \
             LoadCredentialEncrypted=master.key:{} yourself",
            unit.display(),
            sealed.display()
        ),
    }
    eprintln!(
        "\nThen:\n\n  sudo systemctl daemon-reload\n  sudo systemctl restart seedling\n  \
         sudo journalctl -u seedling -n 20 --no-pager\n\nOnce it runs, keep a copy of the key \
         offline and delete the clear file: sudo shred -u {}",
        from.display()
    );
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    match cli.command {
        // Writes files and opens nothing: no logging setup, no key.
        Some(Command::Init(args)) => init_command(cli.config.as_deref(), &args),
        Some(Command::SealKey { from }) => seal_key_command(&from),
        Some(Command::Server(command)) => {
            brigid_server::run(&EDITION, cli.config, Some(command)).await
        }
        None => brigid_server::run(&EDITION, cli.config, None).await,
    }
}
