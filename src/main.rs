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
    Init {
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
    },

    #[command(flatten)]
    Server(brigid_server::Command),
}

/// Handles `seedling init`.
fn init_command(
    config: Option<&Path>,
    domain: &str,
    key_file: &Path,
    no_key_file: bool,
    data_dir: &Path,
    port: u16,
    systemd: bool,
) {
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
        port,
        systemd: executable,
    };
    let written = match machine::init(&options) {
        Ok(written) => written,
        Err(e) => {
            eprintln!("✗ {e}");
            std::process::exit(1);
        }
    };
    for path in &written {
        eprintln!("✓ wrote {}", path.display());
    }
    if options.create_key {
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
    eprintln!(
        "\nThe server listens on 127.0.0.1:{port}. A reverse proxy holds the certificate of {domain} \
         and forwards to it; or edit {} to give the server a certificate of its own.",
        options.config.display()
    );
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

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    match cli.command {
        // Writes files and opens nothing: no logging setup, no key.
        Some(Command::Init {
            domain,
            key_file,
            no_key_file,
            data_dir,
            port,
            systemd,
        }) => init_command(
            cli.config.as_deref(),
            &domain,
            &key_file,
            no_key_file,
            &data_dir,
            port,
            systemd,
        ),
        Some(Command::Server(command)) => {
            brigid_server::run(&EDITION, cli.config, Some(command)).await
        }
        None => brigid_server::run(&EDITION, cli.config, None).await,
    }
}
