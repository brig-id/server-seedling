//! seedling, the edition of brig·id for a machine of its own. The server
//! itself is `brigid-server` (core), the same for every edition; what is
//! seedling's own is here: how it is installed (`machine`: `seedling init`,
//! systemd) and the interface its executable carries.

pub mod machine;

pub use brigid_server::{EmbeddedUi, Ui, UiError, apply_ui};

mod embedded {
    include!(concat!(env!("OUT_DIR"), "/embedded_ui.rs"));
}

/// The interface this executable carries (`build.rs`); none in a
/// development build made without `BRIGID_UI_DIST`.
pub const UI: EmbeddedUi = EmbeddedUi {
    files: embedded::FILES,
    csp_hashes: embedded::CSP_HASHES,
};

/// seedling as an edition of the server.
pub const EDITION: brigid_server::Edition = brigid_server::Edition {
    name: "seedling",
    version: env!("CARGO_PKG_VERSION"),
    ui: UI,
    config_path: Some(machine::CONFIG_PATH),
    key_path: Some(machine::KEY_PATH),
    sealed_key_path: Some(machine::SEALED_KEY_PATH),
    key_hint: ", or create /etc/brigid/master.key with `seedling init`",
    root_hint: ": the systemd unit written by `seedling init --systemd` runs it as `brigid`.",
};
