# AGENTS.md — brig·id `server-seedling`

This repository contains the **single-server deployment binary** for brig·id.
It wires together all `core` crates into a production-ready executable.

## Language

**All content must be in English** — code, comments, doc-comments, commit messages,
issues, pull requests. No exceptions.

## Scope

- Binary `seedling` (`src/main.rs`) — the only entry point. The server itself (configuration,
  operator commands, startup, the embedded interface, metrics) is **`brigid-server` in
  `brig-id/core`**, the same for every edition: seedling describes itself (`EDITION` in
  `src/lib.rs`) and calls `brigid_server::run`. Change behaviour there, not here.
- What is seedling's own: `seedling init` and the systemd files (`src/machine.rs`,
  `deploy/systemd/`), the interface embedded at compile time (`build.rs` calls
  `brigid-ui-embed`, `BRIGID_UI_DIST`; a release build fails without it)
- Docker multi-stage build + distroless image
- The systemd unit (`deploy/systemd/`) and a Compose file for development (`deploy/compose.dev.yaml`)
- E2E smoke tests (`tests/e2e/`)

This repository contains **no business logic**. All logic lives in `brig-id/core`.

## Roadmap & planning

TODOs, backlog ideas, and phase/release status for this repo are tracked as cards in
[brig-id Project 1](https://github.com/orgs/brig-id/projects/1), not in local files.

## Hard security constraints

- **`BRIGID_MASTER_KEY` must never appear in `seedling.toml`** — env var or separate secret file only.
- **Refuse to start** if `MASTER_KEY` is absent or decodes to fewer than 32 bytes.
- **TLS 1.3 only** on the built-in listener (ACME or certificate files) — rustls `ServerConfig`
  in core's `brigid-server`; no OpenSSL for TLS.
  The build container installs `libssl-dev` for `webauthn-rs`'s attestation chain
  validator only (see `core/AGENTS.md` for the documented scope of that exception);
  attestation never touches the TLS stack.
- **Distroless Docker image** (`gcr.io/distroless/cc-debian12`) — no shell, no package manager.
- **Non-root user** — `USER nonroot:nonroot` in the final Docker stage.
- **Read-only container filesystem** — `read_only: true` + tmpfs on `/tmp` wherever the image
  runs (`deploy/compose.dev.yaml` does).
- **Docker secrets** for `BRIGID_MASTER_KEY` — never a plaintext value in compose files.
- **Graceful shutdown** — handle `SIGTERM`/`SIGINT`; SQLite must not be left in a corrupt state.

## Configuration file shape

```toml
[server]
host   = "0.0.0.0"
port   = 8080
domain = "example.com"   # RP ID (WebAuthn) + issuer (OIDC)
acme = true              # or tls_cert/tls_key, or neither behind a proxy
metrics_port = 9100      # optional: internal Prometheus listener (GET /metrics)

[database]
path = "/data/brigid.db"

[security]
# BRIGID_MASTER_KEY comes from env — never here
session_ttl_seconds = 3600        # account session: ends after this long unused
session_lifetime_seconds = 43200  # ...and this long after sign-in in any case
```

## Key crates

- `brigid-api` (core git dep) — Axum application
- `brigid-store` (core git dep) — SQLite init + migrations
- `brigid-crypto` (crypto git dep) — MASTER_KEY loading
- `clap` — CLI argument parsing
- `figment` — config merging (TOML + env)
- `tokio` (full), `tracing-subscriber` (JSON logs)

## Commit conventions

Format: `type(scope): <emoji> description`

| Type | Emoji | When |
| --- | --- | --- |
| `feat` | ✨ | New feature |
| `fix` | 🐛 (🔒️ for security) | Bug fix, security fix or hardening |
| `docs` | 📝 | Documentation only |
| `chore` | 🔧 | Maintenance, config |
| `test` | ✅ | Tests |
| `refactor` | ♻️ | Restructuring, no behaviour change |
| `perf` | ⚡️ | Performance |
| `ci` | 👷 | CI/CD |
| `build` | 📦 | Build system, dependencies |
| `revert` | ⏪ | Reverts a previous commit |

### Allowed scopes

| Scope | Maps to |
| --- | --- |
| `seedling` | `src/main.rs`, binary entry point |
| `config` | Configuration examples (`seedling.example.toml`); the loading is in core's `brigid-server` |
| `docker` | `Dockerfile`, `deploy/` |
| `ci` | `.github/workflows/` |
| `deps` | Dependency bumps |
| `release` | A release pull request (release-please) |

**Do not use a scope outside this list.** If a new source file or concern is added,
update this table and `.vscode/settings.json`.

```text
feat(seedling): ✨ serve Qwik static assets from ui/dist/
fix(config): 🐛 reject partial TLS config at startup
ci(ci): 👷 add conventional commit check
fix(deps): 🐛 bring in brig-id/core to v0.3.0
```

## Releases

Nobody tags by hand and nobody writes the changelog. `.github/workflows/release.yml`
(release-please, through the shared workflow of brig-id/.github) keeps a release pull
request up to date: the next version and its `CHANGELOG.md` entry, from the commits
since the last release. **Merging that pull request is the release**: release-please
creates the tag `vX.Y.Z` and the GitHub Release, then starts `ci.yml` on the tag, whose
tag-only jobs add the static binaries and push `ghcr.io/brig-id/seedling`. So:

- the core, crypto and the interface are pinned by tag (`tag = "vX.Y.Z"`, and `APP_REF`
  with its tag beside it); Renovate proposes their new releases in one pull request, on a
  Monday (sooner from its box on the Dependency Dashboard), always `fix(deps)`, and
  merging it is what opens the next release here; an update from outside the project
  comes as `build(deps)` and waits for the next release;
- the commit type decides the version: while it is 0.x, `feat` and `fix` are a patch,
  a `!` or a `BREAKING CHANGE:` footer a minor; `build`, `ci`, `chore`, `docs`, `test`
  and `refactor` alone make no release;
- the commit subject is the changelog line: write it for someone reading the release;
- do not edit `CHANGELOG.md`, `.release-please-manifest.json` or the version in
  `Cargo.toml` by hand.

## Commands

```bash
cargo build --release -p seedling
docker build -t brigid/seedling .
docker compose -f deploy/compose.dev.yaml up
curl http://localhost:8080/health
seedling healthcheck                     # liveness probe (Docker HEALTHCHECK), exit 0/1
seedling init --domain id.example.org [--systemd]   # a fresh machine: configuration, key, unit (overwrites nothing)
seedling admin link                      # an address that makes the signed-in account that opens it an administrator
seedling signup [open|invitation|closed] # who may create an account: prints it, or sets it
seedling client add|list|attributes|remove   # registered applications
curl http://localhost:9100/metrics   # when server.metrics_port = 9100
```

## Local dev without Docker (paired with `web`'s HTTPS setup)

Faster inner loop than `compose.dev.yaml` when iterating on `seedling` itself — run the
binary directly against the `web` dev server instead of a prebuilt `dist/`. Must match
`web`'s dev origin (`brigid.localhost:5173`, see `web/README.md`'s "HTTPS in dev"
section) or WebAuthn ceremonies fail with an origin/RP-ID mismatch.

```bash
export BRIGID_MASTER_KEY=$(openssl rand -hex 32)   # one-time per shell/machine — any 64 hex chars
export BRIGID_DATABASE__PATH=./brigid-dev.db          # gitignored, auto-created on first run
export BRIGID_SERVER__DOMAIN=brigid.localhost         # must equal web's dev host (WebAuthn RP ID)
export BRIGID_SERVER__PUBLIC_URL=https://brigid.localhost:5173  # https — see web/README.md

cargo build -p seedling
"$CARGO_TARGET_DIR"/debug/seedling
```

Then in `web`: generate the mkcert cert once (`web/README.md`'s "HTTPS in dev"), and
`pnpm dev`. Open `https://brigid.localhost:5173/register/`.

**Currently all manual, no persistence across shells/rebuilds** — `BRIGID_MASTER_KEY`
must be re-exported every session, and the mkcert cert is regenerated per clone. Tracked
as a candidate for automation (a `dev.sh` script, or wiring into the devcontainer's
`postCreateCommand`) in [brig-id Project 1](https://github.com/orgs/brig-id/projects/1).
