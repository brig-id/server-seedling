# Changelog

All notable changes to `server-seedling` (named `server-leaf` up to 0.3.0) are documented
in this file.

From the release after 0.11.0 on, each entry is written by release-please from the subjects
of the Conventional Commits of the release; the entries up to 0.11.0 were written by hand, in
the [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) format. This project adheres
to [Semantic Versioning](https://semver.org/).

## [0.12.2](https://github.com/brig-id/server-seedling/compare/v0.12.1...v0.12.2) (2026-11-27)


### Bug Fixes

* **deps:** 📦 core v0.3.1: backup, restore and limits ([a205b24](https://github.com/brig-id/server-seedling/commit/a205b24d63422284561058d939748c0b1fc9a4b6))

## [0.12.1](https://github.com/brig-id/server-seedling/compare/v0.12.0...v0.12.1) (2026-11-26)


### Reverts

* **deps:** ⏪ core by its tag again; backup and restore wait for it ([571775b](https://github.com/brig-id/server-seedling/commit/571775ba99530db31c35c7c0d642ca49293697d0))

## [0.12.0](https://github.com/brig-id/server-seedling/compare/v0.11.0...v0.12.0) (2026-11-25)


### Features

* **deps:** ✨ bring in core v0.3.0, crypto v0.2.0 and the interface v0.11.1 ([78769da](https://github.com/brig-id/server-seedling/commit/78769da50695aa53272cf6aa0b4e9ae9bc1f2c6d))

## [0.11.0] - 2026-11-25

### Added

- **The certificate obtained by seedling itself** (#75, core#122): `seedling init --acme
  [--acme-contact ADDRESS]` writes a configuration where the server listens alone on port 443,
  obtains its certificate from Let's Encrypt at its first start, proving the domain on that
  same port (TLS-ALPN-01), and renews it. The account and the certificate's key are kept in
  the database, encrypted. In any configuration: `acme = true` (and `acme_directory`,
  `acme_contact`, `acme_ca`). The install guide offers it first.

### Changed

- Core at c2ad77d (sqlx 0.9; every query of the store is fixed at compile time) and the
  interface at app main (9ee1110: the archetypes are the interface's).
- Without a proxy, the server speaks TLS 1.3 only, with certificate files too.
- `seedling.example.toml` lists every setting, the three ways to hold the certificate and
  where the master key comes from.

### Removed

- `deploy/compose.yaml`, `Caddyfile`, `OPENBAO.md` and `load-secrets.sh`: the deployment of
  helm.brig.id, which lives with helm now (it runs grove). seedling installs as one
  executable (`docs/install.md`); the image remains for those who want it.
- `security.cors_origins` (core): the interface is the server's own. A configuration that
  still sets it starts as before.

## [0.10.0] - 2026-11-23

### Added

- **The interface 0.11.0**: a menu under each section (each identity, Compare, New
  identity); the brig·id mark and, for an administrator, two pills to switch between the
  account and the administration; About in both menus. A new identity has a page of its
  own: an archetype creates it in one click, or a form by hand. An identity's page has
  three tabs (Summary with suggestions to set or ignore, My information, Usage). Compare
  warns that the same value in two identities helps data brokers match them.
- **21 archetypes in four groups** (core#121): everyday, work and studies, leisure, online,
  each described by what it is for.

## [0.9.0] - 2026-11-23

### Added

- **The master key sealed by the machine** (#98): `seedling init --systemd --seal`
  seals the new key with `systemd-creds` (the TPM when there is one) in
  `/etc/brigid/master.key.cred`, writes no clear file, and prints the key once to
  keep offline; the unit uses `LoadCredentialEncrypted=`. `seedling seal-key
  [--from FILE]` seals an existing key and switches the unit. A command typed as
  root unseals it (core#115). When `systemd-creds` refuses (no TPM, no host
  key), its own words are what `seal-key` says.
- **Websites with no registration, from the administration** (core#114): an
  administrator chooses which websites that describe themselves are accepted,
  what they may ask for at most, and the accepted and refused lists (page
  **Websites**). On the machine: `seedling client limit --allow … | --all | --none`.
- **The interface 0.10.0**: identities as cards in their own colour, renamed and
  deleted from their own page; **Compare**, the identities side by side, with
  the values two of them share; the administration as a place of its own.

### Changed

- One less library of cryptography in C in the binary (ring, core#116). `url`
  2.5.9 (RUSTSEC-2026-0592).

## [0.8.0] - 2026-11-20

### Added

- **Websites with no registration** (core#113, spec#35): a website that
  publishes a document describing itself
  (`https://…/oauth-client-metadata.json`) and uses its address as its
  `client_id` signs people in with no step on the server and no secret.
  Which of them a server accepts: `seedling client policy open` (any, except
  the refused ones; the default), `listed` (only the accepted ones) or
  `registered` (none); `seedling client accept|refuse|unlist <url>`. The
  install guide's step 7 says so.
- **The page that links an identity to a website names it by its address**
  when the website describes itself: its declared name is shown beside it,
  never instead (interface 0.9.0).

### Changed

- **The server is `brigid-server`, from core** (core#94): configuration, the
  operator commands, startup, the embedded interface and the metrics moved
  there, unchanged, so that grove runs the very same code. seedling keeps
  `init`, its systemd files, and the interface it embeds. Messages that named
  seedling for every command now say "the server" where they are shared.
- **The interface is brig-id/app 0.9.0** (the new mark as favicon, and the
  page that names a website by its address).

## [0.7.1] - 2026-11-17

### Fixed

- **Every passkey of an account carries the same name** (`name@server`) in
  the passkey manager: one added later, or made with the recovery code, was
  named `name` alone (brig-id/core#110). Passkeys already made keep their
  name.

### Changed

- **The version is on the About page**, to anyone: "seedling 0.7.1 ·
  interface 0.7.0". The server says it in `GET /auth/server`.
- **The release files have no version in their names**
  (`seedling-linux-x86_64`, `seedling-linux-aarch64`, `seedling.spdx.json`):
  `releases/latest/download/…` is always the newest, so the guide installs
  and updates with the same commands, and leaves no file behind.
- `seedling init` prints the same start commands as the guide, with `sudo`,
  and warns when its port is taken.
- **`seedling admin link` on a server with no account prints a new setup
  link**, which the running server takes at once: an expired or lost one no
  longer needs a restart. The earlier one stops working.
- The installation guide: what the master key protects, what any reverse
  proxy must do (replace `X-Forwarded-For`), Caddy and nginx as examples or
  no proxy, choosing the port, updating.
- The interface embedded is brig-id/app 0.7.0.

## [0.7.0] - 2026-11-17

Anyone can install it: one executable for any Linux, a command that prepares
the machine, and a guide from download to a person signed in on a website
([docs/install.md](docs/install.md)). A database of 0.6 is used as it is.

### Added

- **The installation guide** (#80), each of its `seedling` commands checked
  by a test against the program.
- **`seedling init`** prepares a machine (brig-id/server-seedling#76): a
  configuration for a server behind a reverse proxy
  (`/etc/brigid/seedling.toml`) and a new master key
  (`/etc/brigid/master.key`, root-only), or none with `--no-key-file` when
  the key comes from another disk. With `--systemd`, the unit
  (`deploy/systemd/seedling.service`, hardened, runs as `brigid`, gets the
  key as a systemd credential) and the system user. It overwrites nothing.
- Without `--config`, the server and its commands read
  `/etc/brigid/seedling.toml` when it exists, and the master key at
  `/etc/brigid/master.key` when the environment names none.
- **Static executables** (#77): each release carries `seedling` for x86-64
  and arm64 Linux, built with musl and linking nothing (OpenSSL vendored), so
  it runs on any distribution. With them: `SHA256SUMS`, an SPDX SBOM and a
  build provenance attestation (`gh attestation verify`).

### Changed

- **The server does not run as root**: it says so and stops. A command typed
  as root on the machine (`seedling client list`) reads the master key, then
  works as the owner of the database's directory, so that no file of the
  database belongs to root.
- **The image is made of the same static binary** (`Dockerfile.release`, on
  `distroless/static`): no C library, no OpenSSL in it. Its arm64 variant no
  longer compiles under emulation, so a release reaches the registry in
  minutes instead of more than an hour. `Dockerfile` still builds the server
  from source.
- The interface embedded is brig-id/app 0.6.0: the same as in 0.6.2, now
  under its released version, which the administration shows.

## [0.6.2] - 2026-11-16

### Changed

- The interface embedded is brig-id/app at `db77277`: **adding a second
  passkey asks where it will be kept** (this device, another phone or
  tablet, a security key), and a place that already holds one for the
  account says so instead of "No passkey was added". Nothing changes in the
  server: it already took several passkeys per account.

## [0.6.1] - 2026-11-16

### Added

- **The version is visible.** `seedling --version` says it on the machine,
  and the server declares its edition and version to the core, which tells
  them to administrators only: the administration shows "This server runs
  seedling 0.6.1, with the interface 0.4.0" at the foot of its pages. No
  public answer names a version.

### Changed

- The interface embedded is brig-id/app at `119b6bc` (0.4.0 and the version
  line).
- `axum-server` 0.9.

## [0.6.0] - 2026-11-16

Administration. A database of 0.5 is upgraded in place (one new schema
script). **After the upgrade a server is by invitation and has no
administrator**: `seedling signup open` keeps it open, and `seedling admin
link` prints the address that makes a signed-in account an administrator.

### Changed

- **Who may create an account is no longer a setting of the configuration.**
  `security.signup_open` is gone (still accepted and ignored, so a deployment
  that sets it starts). The policy is in the database: `open`, `invitation`
  (the default) or `closed`, set by an administrator from the interface or by
  `seedling signup <policy>` on the machine. **A server upgraded from 0.5 is
  by invitation until someone opens it.**

### Added

- **Administration** (brig-id/core#88). A server without account prints, at
  each start, a one-time setup link that creates the first account as
  administrator. A server that has accounts and no administrator gets one
  with `seedling admin link`: the signed-in account that opens the address it
  prints becomes administrator. Administrators see the accounts (never their
  identities), suspend or delete them, grant the role, register applications,
  set who may sign up and make invitations. The secret of a link is in the
  fragment of its address (`/setup/#link=…`): a browser never sends it, so
  it is in no access log, of this server or of a proxy in front of it.
- `security.admin_log_retention_days` (default 365): the log of
  administrative actions names accounts by an identifier only, and entries
  older than this are deleted.
- `security.admin_confirmation_seconds` (default 300): how long a passkey
  ceremony covers administrative changes. Past it, an administrator is asked
  for their passkey again before a change is made.
- The interface embedded is the one with the administration pages
  (brig-id/app at `192db62`, what its 0.4.0 is made of).

## [0.5.0] - 2026-11-15

The first release of step 2. A database of 0.4 is upgraded in place (one new
schema script). An account that has no recovery code is sent to create one at
its next sign-in, before anything else.

### Changed

- **The interface is inside the executable** (#74). It is embedded at compile
  time from the directory named by `BRIGID_UI_DIST`, the text files
  compressed; a release build fails without it, or without its list of script
  hashes. The container image holds the binary and nothing else.
  `server.ui_dist_dir` remains as a development option: a build of the
  interface in that directory is served instead. A directory that cannot be
  served stops a server that has no interface of its own, and is ignored with
  a warning by one that has (so a deployment that still sets it keeps
  working).
- **A path that names nothing answers `404`.** Until now any unknown path was
  answered with the sign-in page and `200`.
- Files named after their content (`/build/`, `/assets/`) are cached for good;
  pages carry an `ETag` and are checked at each visit.
- **No `'unsafe-inline'` any more**: a build of the interface without its
  list of script hashes is not served at all.
- A new account starts with no identity: the onboarding of the interface
  proposes some and creates the ones the person keeps. Until an account has
  its recovery code, its identities and the applications are closed to it
  (brig-id/core#98).

### Added

- A person can delete their account, after a passkey ceremony done for that
  purpose (brig-id/core#98), and download their data first: both are on a new
  "Account" page of the interface (brig-id/app#68).
- The interface (0.3.0) shows, on the page that links an identity to a site,
  each attribute asked for with the value that would be sent, and nothing is
  ticked for the person. Each identity has a page with what it can share and
  what each site was given, where it can be withdrawn (brig-id/app#68).
- A site can ask people for attributes of the identity they present
  (brig-id/core#91): first name, last name, username, e-mail address,
  language, time zone. `seedling client add` takes `--optional <attribute>`
  and `--required <attribute>`, `seedling client attributes <client_id>`
  changes it for a registered site, and `seedling client list` shows it. A
  site asks with the `profile` and `email` scopes; each person sees the values
  and decides. A site registered without any of these options keeps receiving
  an identifier and nothing else, and is refused if it asks for more.

## [0.4.1] - 2026-11-15

### Security

- API responses (JSON and redirects) carry a Content Security Policy that loads
  nothing and names no origin (`default-src 'none'`). They still allowed a
  photo service the interface stopped using (brig-id/core#101).

### Added

- The interface has a light theme, which follows the system until the person
  chooses, with its own embedded background (brig-id/app#66).

## [0.4.0] - 2026-11-14

The first release under the name seedling, and a new start: it opens no
database made by an earlier version.

### Changed

- **This edition is named seedling** (was leaf). The repository is
  `server-seedling`, the binary and its commands `seedling`, the image
  `ghcr.io/brig-id/seedling`, the metrics `brigid_http_*`.
- **Settings are read under the `BRIGID_` prefix** (`BRIGID_SERVER__DOMAIN`…),
  the same for the three editions. The `LEAF_` prefix is no longer read.
- **A server starts again from an empty database.** The schema is one script
  (brig-id/core#100), and a database created by 0.3 or earlier is refused at
  start: nothing deployed had to be kept. With it go the first signing key and
  the VSID secret derived from the master key: both are random, drawn at the
  first start.
- **Licence: AGPL-3.0-or-later** (was LGPL-3.0-or-later). Released versions
  keep the licence they were published under.
- The server's DID document (`/.well-known/did.json`) is no longer served.
- **The Content Security Policy of the interface names no other origin**:
  the font host and the photo API it used to allow are gone, with the
  interface that needed them (brig-id/app#63). `form-action 'self'` is added.
  A test fails if a scheme, a host or a wildcard comes back into it.
- The image is built with no secret: the interface no longer needs a
  registry token nor an API key.
- **The image embeds the rewritten interface** (brig-id/app#63): its own
  design system on Qwik 2, in English and French, with nothing loaded from
  another origin; sign-in at `/`, sign-up at `/signup/`.

### Added

- `security.signup_open` (default `true`): `false` closes the creation of
  accounts; sign-in and recovery are untouched.
- Websites can sign people in through `seedling` (brig-id/core#92):
  `/authorize`, `/token` and `/.well-known/openid-configuration` are served.
  Authorization Code flow with PKCE (`S256`, mandatory), ID tokens signed
  with EdDSA whose only statement about the person is an identifier for that
  site.
- `seedling client add --name <name> --redirect-uri <uri> [--public]`,
  `seedling client list` and `seedling client remove <client_id>` register the sites
  that may do so. `add` prints the `client_id` and, for a site with a secret,
  the `client_secret`, once: only its hash is stored.

## [0.3.0] - 2026-11-13

### Added

- Account recovery with a recovery code (`/account/recovery-code`,
  `/auth/recovery/*`), and passkey-only signup with a generated, hidden
  username, signed in by the same ceremony (brig-id/core#78).
- `security.session_lifetime_seconds` (default 43200, 12 h): an account
  session ends this long after sign-in, used or not.
- The bundled UI is app's current `main`: sign-in with the passkey alone
  (this server or another by domain), one-button signup with an onboarding
  page (passkey name from its provider, recovery code), "I lost my passkeys",
  and no session data in the browser's storage.

### Changed

- **Account sessions live in an HttpOnly cookie** (ADR-005 §1, brig-id/core#80):
  `__Host-brigid_session` over HTTPS, server-side session, no token in the
  browser's storage and no account id in URLs. `security.session_ttl_seconds`
  is now the idle timeout of a session. Requests that change something must
  come from the server's own origin. No token is issued for a caller-supplied
  `client_id` anymore, and `/.well-known/openid-configuration` is gone until
  the relying-party flow exists (ADR-005 §3).
- Registration asks for discoverable passkeys (`residentKey: required`):
  signing in takes no username, so the passkey must name the account.
- Identities and accounts are identified by UUIDs (ADR-007): the database
  migration drops the account and identity DID columns, and VSIDs are derived
  from the identity UUID. No relying party holds a VSID yet.
- `core` update: generic code comes from helpers4. Three checks are stricter:
  the `Bearer` scheme is matched case-insensitively and only RFC 6750 token
  characters are accepted; the server part of a username cannot be an IP
  address; an icon name cannot start or end with a hyphen.

### Upgrading

- Three database migrations run at start (recovery codes, DID columns
  dropped, sessions). **Every session in progress ends: users sign in again.**

## [0.2.1] - 2026-11-11

### Fixed

- The v0.2.0 image did not start: the binary was built on Debian trixie
  (`rust:1.89-slim`, glibc 2.41) for a bookworm runtime (glibc 2.36). The
  builder is pinned to `rust:1.89-slim-bookworm`, and CI now starts the binary
  in the built image.

### Changed

- The VSID secret is stored in the database instead of derived from the
  master key at each start (ADR-006 in the spec). `leaf rotate-key`
  re-encrypts it: **a master key rotation no longer changes every VSID**. An
  existing database keeps the value it was using, so no `sub` changes at the
  upgrade; a new one gets a random secret.

### Fixed

- Accounts can only be registered under this server's own domain, the account
  pages are no longer throttled right after signing in (their rate limit is
  separate from the sign-in ceremonies', 60 requests per minute), and an
  internal error logs its cause (`core` update).
- Rate limiting behind a reverse proxy. Every request reaches `leaf` from the
  proxy's address, so all clients shared one rate-limit bucket (20 requests
  per minute for the whole instance). `security.trust_forwarded_for`
  (`LEAF_SECURITY__TRUST_FORWARDED_FOR`) makes the limit apply per client IP
  read from `X-Forwarded-For`; `deploy/compose.yaml` turns it on. Off by
  default, since a directly reachable `leaf` must not trust that header.

## [0.2.0] - 2026-11-11

### Added

- `leaf healthcheck`: the binary probes its own `/health` (exit 0 on 200, 1
  otherwise), so the distroless image gets a Docker `HEALTHCHECK` without a
  shell or curl. The image and `deploy/compose.yaml` use it; Swarm now sees a
  crash-looping or hung leaf as unhealthy instead of only Caddy's 502s.
- `server.metrics_port` (`LEAF_SERVER__METRICS_PORT`): an internal listener
  serving Prometheus metrics at `/metrics` (request counts by method, matched
  route and status; latency histograms). Off by default; port 9100 on the
  internal overlay network in `deploy/compose.yaml`, never published.
- `leaf rotate-oidc-key`: makes a new OIDC signing key current. Signing keys
  are now stored (encrypted) in the database instead of derived from the
  master key; the previous key keeps verifying and stays in the JWKS for two
  hours, then is removed at a later start. On upgrade the derived key is
  stored under its old `kid` (`v1`), so issued tokens stay valid.
- Identities, templates, passkey names, username-less login, session check and
  refresh, identity tokens: from brig-id/core#72.
- The bundled UI is app's current `main`: signed-in shell with navigation,
  Identities page with an icon library, passkey names, passkey autofill and
  username-less sign-in, clearer unsupported-browser message (brig-id/app#32).

### Changed

- `leaf rotate-key` no longer invalidates issued tokens: the OIDC keys are
  re-encrypted, not re-derived.
- Rust 1.89 (MSRV and the image's builder), as core requires.
- Deleting an account's only passkey is refused (`409`), from core.

### Security

- The UI's Content-Security-Policy allows inline scripts and styles by hash,
  read from the `dist/csp-hashes.json` the UI build now writes, instead of
  `'unsafe-inline'` (core#20, app#3). An older UI build without the manifest
  falls back to `'unsafe-inline'` with a warning in the log.
- `ctutils` 0.4.8 (RUSTSEC-2026-0385, non-constant-time comparisons).

## [0.1.2] - 2026-09-05

### Fixed

- `deploy/compose.yaml`'s `restart: unless-stopped` is plain-Compose-only and
  silently ignored under `docker stack deploy` (Swarm mode); with no
  `deploy.restart_policy`, Swarm fell back to infinite retries every ~5s, so
  a broken deploy crash-looped forever instead of failing loudly. Added a
  bounded `restart_policy` (5 attempts, 5min window) to `leaf` and `caddy`.
- `main.rs` only ever called `MasterKey::from_env()`, never
  `MasterKey::from_file()`, even though `deploy/compose.yaml`'s Docker
  secrets setup (`BRIGID_MASTER_KEY_FILE`) — the only production path
  allowed by this repo's hard security constraints — depended on it. Every
  Docker-secrets deployment failed at startup with "master key environment
  variable not set". `BRIGID_MASTER_KEY_FILE` now takes precedence when set.

## [0.1.1] - 2026-09-04

### Fixed

- `Dockerfile`'s `gcr.io/distroless/cc-debian12` base and `deploy/compose.yaml`'s
  `caddy:2-alpine` image were both pinned to single-platform (amd64) manifest
  digests instead of the multi-arch index, so the arm64 leg of the multi-arch
  release build silently used amd64 base content — `leaf` crashed with
  `exec: no such file or directory` (missing aarch64 interpreter) and `caddy`
  with `exec format error` on arm64 hosts (found deploying to an Oracle
  Ampere A1 instance). Re-pinned to the correct index digests.
- `deploy/compose.yaml` never actually worked under `docker stack deploy`
  despite declaring a Swarm-only `secrets: external: true` — fixed
  `networks.internal.driver` (`bridge` → `overlay`, required for Swarm
  services) and `caddy.depends_on`'s long form (rejected by Swarm's stack
  schema) to the short list form.

## [0.1.0] - 2026-09-04

First tagged release, alongside `crypto`, `core`, and `app`.

### Added

- `leaf` binary: single-server brig·id deployment — loads TOML +
  `LEAF_*` env config via `figment`, requires `BRIGID_MASTER_KEY` (or
  `BRIGID_MASTER_KEY_FILE`) to start, opens/migrates the SQLite database,
  builds the `brigid-api` router, and serves it over TLS 1.3 (or plain
  HTTP for local dev) with graceful `SIGTERM`/Ctrl-C shutdown.
- `leaf rotate-key --old <path> --new <path>`: re-encrypts the entire
  database under a new master key via `brigid-store::rotate_master_key`,
  reading both keys from files only (never `argv`, an env var, or stdin).
  Prints the VSID- and OIDC-signing-key consequences of rotation to
  stderr on success.
- Serves the `brig-id/web` Qwik static (SSG) build as the UI, via
  `ServeDir` with SPA-style fallback for client-side routes.
- Security headers (`X-Content-Type-Options`, `X-Frame-Options`,
  `Strict-Transport-Security`, `Content-Security-Policy`) applied as the
  outermost layer, covering both the API and the static UI fallback.
- Docker: multi-stage build (Rust builder + `gcr.io/distroless/cc-debian12`
  runtime), non-root (`nonroot:nonroot`), read-only-filesystem-compatible,
  with a pre-chowned `/data` directory for the SQLite file.
- `docker-compose` deployment with a Caddy reverse proxy in front, TLS
  termination, and a smoke-test script (`smoke.sh`) exercising the
  discovery/health endpoints end to end.
- E2E smoke tests (`tests/smoke.rs`): spawn a real `leaf` subprocess and
  drive it over HTTP with a software WebAuthn authenticator — register,
  login, list/delete passkeys, and rate-limit enforcement (the 21st
  `/auth/*` request in a window returns `429`).
- `tests/rotate_key.rs`: full CLI lifecycle test — register under the old
  key over real HTTP, stop the server, run `rotate-key`, confirm the old
  key file no longer works, restart under the new key, and log in again.
- `tests/binary.rs`: startup/shutdown behavior — missing `MASTER_KEY`,
  missing config file, a valid config actually listens, graceful
  `SIGTERM` shutdown, and the port-already-in-use case.

### Fixed

- `SetResponseHeaderLayer`-based security headers previously missed the
  static-UI fallback route because they were applied before
  `.fallback_service()` in the Axum 0.8 layer stack (layers only cover
  routes that exist at the time `.layer()` is called); now applied after.
- A missing `--config` file was silently treated as "no config" by
  `figment` instead of failing loudly; `leaf` now checks the path exists
  before attempting to load it.
- `rotate-key`'s database-path extraction was reading from a figment tree
  shaped for the full `Config`, silently falling back to the default path
  instead of `LEAF_DATABASE__PATH`; fixed via an explicit `.focus("database")`.
- Distroless runtime's fresh named Docker volume for `/data` inherited
  `root:root` ownership, which the non-root container user couldn't write
  to; fixed by pre-creating and chowning `/data` in the builder stage and
  copying it into the runtime stage.
- `rust:1.85-slim` was too old to build the pinned `time@0.3.47`;
  repinned to `rust:1.88-slim` (last Debian bookworm tag), matching the
  distroless runtime's glibc.

### Security

- CSP currently allows `'unsafe-inline'` for `script-src`/`style-src` as a
  tracked stopgap — the Qwik SSG build emits inline scripts/styles that a
  strict CSP would otherwise block. Tracked in `.dev/phases/backlog.md`;
  the real fix is a build-time SHA-256 hash allowlist.
- `/auth/*` is rate-limited to a sustained 20 requests/minute per IP
  (burst 5, refill every 3s), keyed on the real TCP peer address unless a
  trusted reverse proxy is configured to set `x-forwarded-for`.
