//! E2E smoke tests (phase-3 "E2E smoke tests (Rust + reqwest)" checklist).
//!
//! Unlike `tests/static_files.rs` (in-process `axum::Router`) and
//! `tests/binary.rs` (process lifecycle only), these spawn a real `seedling`
//! subprocess and drive it over real HTTP with `reqwest`, including a full
//! WebAuthn ceremony via a software passkey — the closest thing to what a
//! real client does, short of a browser.

mod common;

use std::{
    fs,
    net::SocketAddr,
    process::{Child, Command, Stdio},
    time::Duration,
};

use common::{
    TEST_MASTER_KEY, free_port, open_sign_up, seedling, seedling_bin, wait_until_listening,
};
use serde_json::{Value, json};
use webauthn_authenticator_rs::{WebauthnAuthenticator, softpasskey::SoftPasskey};
use webauthn_rs::prelude::{CreationChallengeResponse, RequestChallengeResponse};

struct TestServer {
    child: Child,
    pub base_url: String,
    pub client: reqwest::Client,
    pub db_path: std::path::PathBuf,
    _db_dir: tempfile::TempDir,
    _ui_dir: tempfile::TempDir,
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.child.kill().ok();
        self.child.wait().ok();
    }
}

/// Spawns `seedling` with a fresh temp DB and a minimal build of the
/// interface (a page and its list of script hashes), waits for it to start listening,
/// and returns a client pointed at it. Each call gets its own process — and
/// so its own in-memory rate-limit bucket — which is what keeps
/// `rate_limit_21st_auth_request_returns_429` from bleeding into the other
/// tests here.
fn start_seedling() -> TestServer {
    start_seedling_with(&[])
}

/// [`start_seedling`] with extra environment variables for the `seedling`
/// process. Anyone can create an account on it.
fn start_seedling_with(extra_env: &[(&str, &str)]) -> TestServer {
    start(extra_env, true).0
}

/// Starts `seedling`. `open`: sign-up is opened first; else the server is as
/// an operator finds it, and what it says on standard error is returned.
fn start(
    extra_env: &[(&str, &str)],
    open: bool,
) -> (TestServer, Option<std::process::ChildStderr>) {
    let port = free_port();
    let db_dir = tempfile::TempDir::new().expect("tempdir for db");
    let db_path = db_dir.path().join("smoke.db");
    if open {
        open_sign_up(&db_path);
    }

    let ui_dir = tempfile::TempDir::new().expect("tempdir for ui dist");
    fs::write(
        ui_dir.path().join("index.html"),
        b"<!DOCTYPE html><html><body>smoke-test-ui</body></html>",
    )
    .expect("write fake index.html");
    // Without its list of script hashes a directory is not served at all.
    fs::write(
        ui_dir.path().join("csp-hashes.json"),
        br#"{"script":["'sha256-AAAA'"],"style":[]}"#,
    )
    .expect("write the list of hashes");

    let mut child = Command::new(seedling_bin())
        .env("BRIGID_MASTER_KEY", TEST_MASTER_KEY)
        .env("BRIGID_SERVER__DOMAIN", "localhost")
        .env("BRIGID_SERVER__HOST", "127.0.0.1")
        .env("BRIGID_SERVER__PORT", port.to_string())
        .env("BRIGID_DATABASE__PATH", db_path.display().to_string())
        .env(
            "BRIGID_SERVER__UI_DIST_DIR",
            ui_dir.path().display().to_string(),
        )
        .envs(extra_env.iter().copied())
        .stdout(Stdio::null())
        .stderr(if open { Stdio::null() } else { Stdio::piped() })
        .spawn()
        .expect("spawn seedling");
    let said = child.stderr.take();

    let addr: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
    if !wait_until_listening(addr, Duration::from_secs(5)) {
        // `child` isn't wrapped in `TestServer` yet, so its `Drop` (which
        // would kill it) never runs — without this, a failed startup leaks
        // the orphaned `seedling` process instead of just failing the test.
        child.kill().ok();
        child.wait().ok();
        panic!("seedling did not start listening");
    }

    let server = TestServer {
        child,
        // WebAuthn strictly checks the ceremony's origin against the RP's
        // configured domain (`BRIGID_SERVER__DOMAIN=localhost` above) — using
        // 127.0.0.1 here would mismatch and fail with a `Security` error.
        // "localhost" resolves to 127.0.0.1 via /etc/hosts either way.
        base_url: format!("http://localhost:{port}"),
        // A browser sends its page's origin on every request that changes
        // something, and the API refuses any other (ADR-005).
        client: reqwest::Client::builder()
            .default_headers(
                [(
                    reqwest::header::ORIGIN,
                    reqwest::header::HeaderValue::from_str(&format!("http://localhost:{port}"))
                        .unwrap(),
                )]
                .into_iter()
                .collect(),
            )
            .build()
            .unwrap(),
        db_path,
        _db_dir: db_dir,
        _ui_dir: ui_dir,
    };
    (server, said)
}

/// The session cookie (`name=value`) a response sets; checks its attributes.
fn session_cookie(resp: &reqwest::Response) -> String {
    let set = resp
        .headers()
        .get(reqwest::header::SET_COOKIE)
        .expect("the response opens a session")
        .to_str()
        .unwrap();
    assert!(
        set.contains("HttpOnly") && set.contains("SameSite=Strict"),
        "{set}"
    );
    set.split(';').next().unwrap().to_string()
}

async fn json_body(resp: reqwest::Response) -> Value {
    resp.json().await.expect("valid JSON body")
}

// ---------------------------------------------------------------------------
// Health & discovery
// ---------------------------------------------------------------------------

#[tokio::test]
async fn health_returns_200() {
    let server = start_seedling();
    let resp = server
        .client
        .get(format!("{}/health", server.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body = json_body(resp).await;
    assert_eq!(body["status"], "ok");
}

#[tokio::test]
async fn openid_configuration_describes_the_code_flow() {
    let server = start_seedling();
    let resp = server
        .client
        .get(format!(
            "{}/.well-known/openid-configuration",
            server.base_url
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body = json_body(resp).await;
    assert_eq!(body["issuer"], server.base_url);
    assert_eq!(
        body["authorization_endpoint"],
        format!("{}/authorize", server.base_url)
    );
    assert_eq!(body["token_endpoint"], format!("{}/token", server.base_url));
    assert_eq!(
        body["id_token_signing_alg_values_supported"],
        json!(["EdDSA"])
    );
}

#[tokio::test]
async fn jwks_has_non_empty_keys() {
    let server = start_seedling();
    let resp = server
        .client
        .get(format!("{}/.well-known/jwks.json", server.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body = json_body(resp).await;
    let keys = body["keys"].as_array().expect("`keys` should be an array");
    assert!(!keys.is_empty(), "expected at least one key: {body}");
}

#[tokio::test]
async fn the_interface_is_served_and_an_unknown_path_is_not_a_page() {
    let server = start_seedling();
    let resp = server
        .client
        .get(format!("{}/", server.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let header = |name: &str| {
        resp.headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_string()
    };
    assert!(header("content-type").contains("text/html"));
    // Under the policy made of the build's hashes, and no weaker one.
    let csp = header("content-security-policy");
    assert!(csp.contains("script-src 'self' 'sha256-AAAA'"), "{csp}");
    assert!(!csp.contains("unsafe-inline"), "{csp}");
    assert!(resp.text().await.unwrap().contains("smoke-test-ui"));

    // No page answers for the paths that name nothing.
    let resp = server
        .client
        .get(format!("{}/login", server.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
}

/// A directory that is not a build of the interface (here: without its list
/// of script hashes) stops a server that carries no interface of its own.
/// One that does carries on with its own: this test is about the first.
#[test]
fn a_directory_without_its_list_of_hashes_is_not_served() {
    if seedling::Ui::embedded(&seedling::UI).is_some() {
        return;
    }
    let ui_dir = tempfile::TempDir::new().expect("tempdir");
    fs::write(ui_dir.path().join("index.html"), b"<html></html>").unwrap();
    let db_dir = tempfile::TempDir::new().expect("tempdir");
    let out = Command::new(seedling_bin())
        .env("BRIGID_MASTER_KEY", TEST_MASTER_KEY)
        .env("BRIGID_SERVER__DOMAIN", "localhost")
        .env("BRIGID_SERVER__HOST", "127.0.0.1")
        .env("BRIGID_SERVER__PORT", free_port().to_string())
        .env(
            "BRIGID_DATABASE__PATH",
            db_dir.path().join("smoke.db").display().to_string(),
        )
        .env(
            "BRIGID_SERVER__UI_DIST_DIR",
            ui_dir.path().display().to_string(),
        )
        .output()
        .expect("run seedling");
    assert!(!out.status.success(), "the server must not start");
    let said = String::from_utf8_lossy(&out.stderr);
    assert!(
        said.contains("server.ui_dist_dir") && said.contains("csp-hashes.json"),
        "{said}"
    );
}

// ---------------------------------------------------------------------------
// WebAuthn registration + login round trip, OIDC token claims
// ---------------------------------------------------------------------------

#[tokio::test]
async fn webauthn_register_and_login_round_trip_opens_a_session() {
    let server = start_seedling();
    let origin = url::Url::parse(&server.base_url).unwrap();
    let mut auth_client = WebauthnAuthenticator::new(SoftPasskey::new(true));
    let username = "smoke-alice@localhost";

    // -- register/begin --
    let resp = server
        .client
        .post(format!("{}/auth/register/begin", server.base_url))
        .json(&json!({ "username": username }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "register/begin should succeed");
    let begin = json_body(resp).await;
    let session_id = begin["session_id"].clone();
    let ccr: CreationChallengeResponse = serde_json::from_value(begin["challenge"].clone())
        .expect("challenge should deserialize as CreationChallengeResponse");

    // -- perform the ceremony with a software passkey --
    let credential = auth_client
        .do_registration(origin.clone(), for_soft_passkey(ccr))
        .expect("software passkey registration ceremony");

    // -- register/finish --
    let resp = server
        .client
        .post(format!("{}/auth/register/finish", server.base_url))
        .json(&json!({ "session_id": session_id, "credential": credential }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 204, "register/finish should succeed");

    // -- login/begin --
    let resp = server
        .client
        .post(format!("{}/auth/login/begin", server.base_url))
        .json(&json!({ "username": username }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "login/begin should succeed");
    let begin = json_body(resp).await;
    let login_session_id = begin["session_id"].clone();
    let rcr: RequestChallengeResponse = serde_json::from_value(begin["challenge"].clone())
        .expect("challenge should deserialize as RequestChallengeResponse");

    let assertion = auth_client
        .do_authentication(origin, rcr)
        .expect("software passkey authentication ceremony");

    // -- login/finish --
    let resp = server
        .client
        .post(format!("{}/auth/login/finish", server.base_url))
        .json(&json!({ "session_id": login_session_id, "credential": assertion }))
        .send()
        .await
        .unwrap();
    // A session in an HttpOnly cookie, never a token (ADR-005).
    assert_eq!(resp.status(), 204, "login/finish should succeed");
    let cookie = session_cookie(&resp);
    assert!(
        resp.text().await.unwrap().is_empty(),
        "no token in the body"
    );
    let resp = server
        .client
        .get(format!("{}/auth/session", server.base_url))
        .header(reqwest::header::COOKIE, &cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "the session cookie opens the account");
}

// ---------------------------------------------------------------------------
// Delete passkey after login
// ---------------------------------------------------------------------------

#[tokio::test]
async fn deleting_the_only_passkey_is_refused() {
    let server = start_seedling();
    let origin = url::Url::parse(&server.base_url).unwrap();
    let mut auth_client = WebauthnAuthenticator::new(SoftPasskey::new(true));
    let username = "smoke-bob@localhost";

    let resp = server
        .client
        .post(format!("{}/auth/register/begin", server.base_url))
        .json(&json!({ "username": username }))
        .send()
        .await
        .unwrap();
    let begin = json_body(resp).await;
    let ccr: CreationChallengeResponse =
        serde_json::from_value(begin["challenge"].clone()).unwrap();
    let credential = auth_client
        .do_registration(origin.clone(), for_soft_passkey(ccr))
        .unwrap();
    server
        .client
        .post(format!("{}/auth/register/finish", server.base_url))
        .json(&json!({ "session_id": begin["session_id"], "credential": credential }))
        .send()
        .await
        .unwrap();

    let resp = server
        .client
        .post(format!("{}/auth/login/begin", server.base_url))
        .json(&json!({ "username": username, }))
        .send()
        .await
        .unwrap();
    let begin = json_body(resp).await;
    let rcr: RequestChallengeResponse = serde_json::from_value(begin["challenge"].clone()).unwrap();
    let assertion = auth_client.do_authentication(origin, rcr).unwrap();
    let resp = server
        .client
        .post(format!("{}/auth/login/finish", server.base_url))
        .json(&json!({ "session_id": begin["session_id"], "credential": assertion }))
        .send()
        .await
        .unwrap();
    let cookie = session_cookie(&resp);

    // List passkeys to get a real credential id to delete.
    let resp = server
        .client
        .get(format!("{}/auth/passkeys", server.base_url))
        .header(reqwest::header::COOKIE, &cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let passkeys = json_body(resp).await;
    let passkey_id = passkeys[0]["id"]
        .as_str()
        .expect("expected at least one registered passkey")
        .to_string();

    // This is the 6th /auth/* call in this test (register begin/finish,
    // login begin/finish, list) against a burst of 5 (1 token/3s refill) —
    // a real rate-limit constraint, not a bug. Give it a moment to refill
    // rather than weakening the limiter to make the test faster.
    tokio::time::sleep(Duration::from_secs(3)).await;

    let resp = server
        .client
        .delete(format!("{}/auth/passkeys/{passkey_id}", server.base_url))
        .header(reqwest::header::COOKIE, &cookie)
        .send()
        .await
        .unwrap();
    // brig·id is passkey-only: removing the account's last passkey would
    // lock it out, so the server refuses (core#41).
    assert_eq!(resp.status(), 409, "the only passkey must not be deletable");
}

// ---------------------------------------------------------------------------
// Rate limiting
// ---------------------------------------------------------------------------

#[tokio::test]
async fn twenty_first_auth_request_returns_429() {
    let server = start_seedling();
    // GovernorLayer wraps the whole /auth/* router ahead of the handlers, so
    // a request that 404s for an unknown user still consumes a token — no
    // need for a valid ceremony here, just enough distinct requests fired
    // fast enough that the bucket's 1-token/3s refill can't keep up.
    let mut last_status = reqwest::StatusCode::OK;
    for _ in 0..21 {
        last_status = server
            .client
            .post(format!("{}/auth/login/begin", server.base_url))
            .json(&json!({ "username": "nobody@localhost", }))
            .send()
            .await
            .unwrap()
            .status();
    }
    assert_eq!(
        last_status, 429,
        "21st rapid /auth/* request should be rate-limited"
    );
}

/// Sends `count` rapid `/auth/login/begin` requests announcing `client_ip` in
/// `X-Forwarded-For`, as a reverse proxy would, and returns the last status.
async fn hammer_login_begin(server: &TestServer, client_ip: &str, count: usize) -> u16 {
    let mut last_status = 0;
    for _ in 0..count {
        last_status = server
            .client
            .post(format!("{}/auth/login/begin", server.base_url))
            .header("x-forwarded-for", client_ip)
            .json(&json!({ "username": "nobody@localhost", }))
            .send()
            .await
            .unwrap()
            .status()
            .as_u16();
    }
    last_status
}

#[tokio::test]
async fn behind_a_trusted_proxy_each_client_has_its_own_rate_limit() {
    let server = start_seedling_with(&[("BRIGID_SECURITY__TRUST_FORWARDED_FOR", "true")]);
    assert_eq!(hammer_login_begin(&server, "203.0.113.1", 21).await, 429);
    // A second client behind the same proxy is not throttled by the first.
    assert_ne!(hammer_login_begin(&server, "203.0.113.2", 1).await, 429);
}

#[tokio::test]
async fn without_a_trusted_proxy_forwarded_for_does_not_split_the_rate_limit() {
    let server = start_seedling();
    assert_eq!(hammer_login_begin(&server, "203.0.113.1", 21).await, 429);
    // The header is ignored: forging it must not buy a fresh bucket.
    assert_eq!(hammer_login_begin(&server, "203.0.113.2", 1).await, 429);
}

/// The server asks for a discoverable passkey (`residentKey: required`);
/// `SoftPasskey` keeps no resident keys and would refuse. Check the request,
/// then relax it for the software authenticator.
fn for_soft_passkey(mut ccr: CreationChallengeResponse) -> CreationChallengeResponse {
    let selection = ccr
        .public_key
        .authenticator_selection
        .as_mut()
        .expect("registration states an authenticator selection");
    assert!(
        selection.require_resident_key,
        "discoverable passkey requested"
    );
    selection.resident_key = Some(webauthn_rs_proto::ResidentKeyRequirement::Discouraged);
    selection.require_resident_key = false;
    ccr
}

// ---------------------------------------------------------------------------
// Relying parties: `seedling client`, then the whole sign-in (ADR-005 §2)
// ---------------------------------------------------------------------------

/// Runs `seedling client …` on the server's database and returns its standard
/// output, with whether it succeeded.
fn seedling_client(server: &TestServer, args: &[&str]) -> (bool, String) {
    let out = Command::new(seedling_bin())
        .arg("client")
        .args(args)
        .env("BRIGID_MASTER_KEY", TEST_MASTER_KEY)
        .env(
            "BRIGID_DATABASE__PATH",
            server.db_path.display().to_string(),
        )
        .output()
        .expect("run seedling client");
    (
        out.status.success(),
        String::from_utf8(out.stdout).expect("UTF-8 output"),
    )
}

/// The value of `key=` in the output of `seedling client add`.
fn output_value(stdout: &str, key: &str) -> Option<String> {
    stdout
        .lines()
        .find_map(|line| line.strip_prefix(&format!("{key}=")))
        .map(str::to_owned)
}

#[tokio::test]
async fn a_site_registered_from_the_command_line_signs_a_person_in() {
    use base64ct::{Base64UrlUnpadded, Encoding};

    // RFC 7636 appendix B.
    const VERIFIER: &str = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
    const CHALLENGE: &str = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
    const REDIRECT: &str = "https://shop.example/cb";

    let server = start_seedling();
    let base = &server.base_url;

    // -- the operator registers the site, on the database of the running server --
    let (ok, out) = seedling_client(
        &server,
        &["add", "--name", "Shop", "--redirect-uri", REDIRECT],
    );
    assert!(ok, "seedling client add: {out}");
    let client_id = output_value(&out, "client_id").expect("client_id is printed");
    let secret = output_value(&out, "client_secret").expect("a secret is printed once");
    let (ok, out) = seedling_client(&server, &["list"]);
    assert!(
        ok && out.contains(&client_id) && out.contains("Shop"),
        "{out}"
    );
    assert!(!out.contains(&secret), "the secret is never shown again");
    // A redirect URI that is not https is refused.
    let (ok, _) = seedling_client(
        &server,
        &[
            "add",
            "--name",
            "Bad",
            "--redirect-uri",
            "http://shop.example/cb",
        ],
    );
    assert!(!ok);

    // -- a person signs up: the session is in a cookie --
    let origin = url::Url::parse(base).unwrap();
    let mut passkey = WebauthnAuthenticator::new(SoftPasskey::new(true));
    let resp = server
        .client
        .post(format!("{base}/auth/register/begin"))
        .json(&json!({ "username": "smoke-site@localhost" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let begin = json_body(resp).await;
    let ccr: CreationChallengeResponse =
        serde_json::from_value(begin["challenge"].clone()).unwrap();
    let credential = passkey
        .do_registration(origin, for_soft_passkey(ccr))
        .expect("registration ceremony");
    let resp = server
        .client
        .post(format!("{base}/auth/register/finish"))
        .json(&json!({ "session_id": begin["session_id"], "credential": credential }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 204);
    let cookie = session_cookie(&resp);
    // The onboarding: a recovery code first, without which the account can do
    // nothing else, then an identity. The server makes none by itself.
    let closed = server
        .client
        .get(format!("{base}/account/identities"))
        .header(reqwest::header::COOKIE, &cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(closed.status(), 403, "no recovery code yet");
    let resp = server
        .client
        .post(format!("{base}/account/recovery-code"))
        .header(reqwest::header::COOKIE, &cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let listed = json_body(
        server
            .client
            .get(format!("{base}/account/identities"))
            .header(reqwest::header::COOKIE, &cookie)
            .send()
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(listed, json!([]), "the server creates no identity");
    let created = json_body(
        server
            .client
            .post(format!("{base}/account/identities"))
            .header(reqwest::header::COOKIE, &cookie)
            .json(&json!({ "template": "personal" }))
            .send()
            .await
            .unwrap(),
    )
    .await;
    let identity = created["id"].as_str().expect("an identity").to_owned();
    let identity = identity.as_str();

    // -- the site sends the browser: no cookie, no Origin, no redirect followed --
    let browser = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let authorize = |client_id: &str| {
        format!(
            "{base}/authorize?response_type=code&scope=openid&client_id={client_id}\
             &redirect_uri=https%3A%2F%2Fshop.example%2Fcb&state=xyz&nonce=n-0S6\
             &code_challenge={CHALLENGE}&code_challenge_method=S256"
        )
    };
    let resp = browser.get(authorize(&client_id)).send().await.unwrap();
    assert_eq!(resp.status(), 303);
    let location = resp.headers()["location"].to_str().unwrap().to_owned();
    let request = location
        .strip_prefix("/continue/?request=")
        .unwrap_or_else(|| panic!("unexpected redirect: {location}"));

    // -- the linking page: the site's name, then the person's choice --
    let view = json_body(
        server
            .client
            .get(format!("{base}/auth/authorize/{request}"))
            .header(reqwest::header::COOKIE, &cookie)
            .send()
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(view["client"]["name"], "Shop");
    assert_eq!(view["must_sign_in"], false);
    let resp = server
        .client
        .post(format!("{base}/auth/authorize/{request}/approve"))
        .header(reqwest::header::COOKIE, &cookie)
        .json(&json!({ "identity_id": identity }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let answer = json_body(resp).await;
    let back = url::Url::parse(answer["redirect_to"].as_str().unwrap()).unwrap();
    assert!(back.as_str().starts_with(REDIRECT), "{back}");
    let query: std::collections::HashMap<_, _> = back.query_pairs().into_owned().collect();
    assert_eq!(query["state"], "xyz");
    assert_eq!(query["iss"], *base);

    // -- the site exchanges the code, with its secret and the PKCE verifier --
    let form = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs([
            ("grant_type", "authorization_code"),
            ("code", query["code"].as_str()),
            ("redirect_uri", REDIRECT),
            ("code_verifier", VERIFIER),
        ])
        .finish();
    let resp = browser
        .post(format!("{base}/token"))
        .basic_auth(&client_id, Some(&secret))
        .header(
            reqwest::header::CONTENT_TYPE,
            "application/x-www-form-urlencoded",
        )
        .body(form.clone())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let tokens = json_body(resp).await;
    let id_token = tokens["id_token"].as_str().expect("an ID token");
    let payload = id_token.split('.').nth(1).expect("a JWT");
    let claims: Value =
        serde_json::from_slice(&Base64UrlUnpadded::decode_vec(payload).unwrap()).unwrap();
    assert_eq!(claims["iss"], *base);
    assert_eq!(claims["aud"], client_id);
    assert_eq!(claims["nonce"], "n-0S6");
    assert_eq!(claims["sub"].as_str().unwrap().len(), 43, "a VSID");
    // The identifier and the token's own fields, nothing about the identity.
    let mut names: Vec<&str> = claims
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    names.sort_unstable();
    assert_eq!(
        names,
        [
            "aud",
            "auth_time",
            "exp",
            "iat",
            "iss",
            "jti",
            "nonce",
            "sub"
        ]
    );

    // The code worked once; a wrong secret gets nothing.
    let resp = browser
        .post(format!("{base}/token"))
        .basic_auth(&client_id, Some(&secret))
        .header(
            reqwest::header::CONTENT_TYPE,
            "application/x-www-form-urlencoded",
        )
        .body(form.clone())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
    let resp = browser
        .post(format!("{base}/token"))
        .basic_auth(&client_id, Some("wrong"))
        .header(
            reqwest::header::CONTENT_TYPE,
            "application/x-www-form-urlencoded",
        )
        .body(form.clone())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);

    // -- the operator lets the site ask for attributes; the person decides --
    let (ok, out) = seedling_client(&server, &["list"]);
    assert!(
        ok && out.trim_end().ends_with("\t-"),
        "nothing so far: {out}"
    );
    for wrong in [
        &["--required", "phone_number"][..],
        &["--required", "email", "--optional", "email"][..],
    ] {
        let (ok, _) = seedling_client(&server, &[&["attributes", &client_id], wrong].concat());
        assert!(!ok, "{wrong:?}");
    }
    let (ok, _) = seedling_client(
        &server,
        &["attributes", "no-such-site", "--required", "email"],
    );
    assert!(!ok);
    let (ok, _) = seedling_client(
        &server,
        &[
            "attributes",
            &client_id,
            "--required",
            "email",
            "--optional",
            "given_name",
        ],
    );
    assert!(ok);
    let (_, out) = seedling_client(&server, &["list"]);
    assert!(out.contains("given_name=optional,email=required"), "{out}");

    let resp = server
        .client
        .patch(format!("{base}/account/identities/{identity}"))
        .header(reqwest::header::COOKIE, &cookie)
        .json(&json!({ "attributes": { "email": "ada@example.org", "given_name": "Ada" } }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let resp = browser
        .get(authorize(&client_id).replace("scope=openid", "scope=openid+profile+email"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 303);
    let location = resp.headers()["location"].to_str().unwrap().to_owned();
    let request = location
        .strip_prefix("/continue/?request=")
        .unwrap_or_else(|| panic!("unexpected redirect: {location}"));
    let view = json_body(
        server
            .client
            .get(format!("{base}/auth/authorize/{request}"))
            .header(reqwest::header::COOKIE, &cookie)
            .send()
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(
        view["asked"],
        json!([
            { "name": "given_name", "required": false },
            { "name": "email", "required": true },
        ])
    );
    // The address is required: refusing it does not sign in.
    let approve = |attributes: Value| {
        server
            .client
            .post(format!("{base}/auth/authorize/{request}/approve"))
            .header(reqwest::header::COOKIE, &cookie)
            .json(&json!({ "identity_id": identity, "attributes": attributes }))
            .send()
    };
    assert_eq!(approve(json!([])).await.unwrap().status(), 409);
    // The first name is optional: refused, the sign-in goes on without it.
    let resp = approve(json!(["email"])).await.unwrap();
    assert_eq!(resp.status(), 200);
    let answer = json_body(resp).await;
    let back = url::Url::parse(answer["redirect_to"].as_str().unwrap()).unwrap();
    let query: std::collections::HashMap<_, _> = back.query_pairs().into_owned().collect();
    let form = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs([
            ("grant_type", "authorization_code"),
            ("code", query["code"].as_str()),
            ("redirect_uri", REDIRECT),
            ("code_verifier", VERIFIER),
        ])
        .finish();
    let resp = browser
        .post(format!("{base}/token"))
        .basic_auth(&client_id, Some(&secret))
        .header(
            reqwest::header::CONTENT_TYPE,
            "application/x-www-form-urlencoded",
        )
        .body(form)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let tokens = json_body(resp).await;
    let payload = tokens["id_token"]
        .as_str()
        .unwrap()
        .split('.')
        .nth(1)
        .unwrap();
    let claims: Value =
        serde_json::from_slice(&Base64UrlUnpadded::decode_vec(payload).unwrap()).unwrap();
    assert_eq!(claims["email"], "ada@example.org");
    assert_eq!(claims["email_verified"], false);
    assert!(claims.get("given_name").is_none(), "refused: {claims}");

    // Without any option, the site is back to an identifier only: asking
    // for more is refused, and the site is told.
    let (ok, _) = seedling_client(&server, &["attributes", &client_id]);
    assert!(ok);
    let resp = browser
        .get(authorize(&client_id).replace("scope=openid", "scope=openid+email"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 303);
    let location = resp.headers()["location"].to_str().unwrap();
    assert!(
        location.starts_with(REDIRECT) && location.contains("error=invalid_scope"),
        "{location}"
    );

    // -- the operator removes the site: its requests stay on this server --
    let (ok, _) = seedling_client(&server, &["remove", &client_id]);
    assert!(ok);
    let resp = browser.get(authorize(&client_id)).send().await.unwrap();
    assert_eq!(resp.status(), 303);
    assert_eq!(
        resp.headers()["location"],
        "/continue/?error=unknown_client"
    );
    let (ok, _) = seedling_client(&server, &["remove", &client_id]);
    assert!(!ok, "removing it twice fails");
}

/// The first line of `text` that holds `needle`, from `needle` to its end.
fn address_in(text: &str, needle: &str) -> Option<String> {
    text.split_whitespace()
        .find(|word| word.contains(needle))
        .map(str::to_owned)
}

/// On the machine, the executable says which version it is. In the
/// interface the same is told to administrators (brig-id/core).
#[test]
fn the_executable_says_its_version() {
    let said = std::process::Command::new(env!("CARGO_BIN_EXE_seedling"))
        .arg("--version")
        .output()
        .expect("seedling runs");
    assert!(said.status.success());
    assert_eq!(
        String::from_utf8_lossy(&said.stdout).trim(),
        concat!("seedling ", env!("CARGO_PKG_VERSION"))
    );
}

#[tokio::test]
async fn a_new_server_takes_its_first_administrator_from_the_link_it_prints() {
    use std::io::{BufRead, BufReader};

    let (server, said) = start(&[], false);
    let base = &server.base_url;

    // Nobody opened it: no account without a link.
    let info = json_body(
        server
            .client
            .get(format!("{base}/auth/server"))
            .send()
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(info["policy"], "invitation");
    assert_eq!(info["sign_up"], false);
    let resp = server
        .client
        .post(format!("{base}/auth/register/begin"))
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 403);
    assert_eq!(json_body(resp).await["code"], "invitation_required");

    // What the operator reads where the server was started.
    let mut lines = BufReader::new(said.expect("standard error")).lines();
    let address = loop {
        let line = lines
            .next()
            .expect("the server prints its setup link")
            .unwrap();
        if let Some(address) = address_in(&line, "/setup/#link=") {
            break address;
        }
    };
    assert!(
        address.starts_with(&format!("{base}/setup/#link=")),
        "{address}"
    );
    let link = address.split("link=").nth(1).unwrap().to_owned();

    // Used, it makes the first account, an administrator; once.
    let origin = url::Url::parse(base).unwrap();
    let mut passkey = WebauthnAuthenticator::new(SoftPasskey::new(true));
    let sign_up = async |passkey: &mut WebauthnAuthenticator<SoftPasskey>, link: &str| {
        let resp = server
            .client
            .post(format!("{base}/auth/register/begin"))
            .json(&json!({ "link": link }))
            .send()
            .await
            .unwrap();
        if resp.status() != 200 {
            return Err(json_body(resp).await);
        }
        let begin = json_body(resp).await;
        let ccr: CreationChallengeResponse =
            serde_json::from_value(begin["challenge"].clone()).unwrap();
        let credential = passkey
            .do_registration(origin.clone(), for_soft_passkey(ccr))
            .expect("registration ceremony");
        let resp = server
            .client
            .post(format!("{base}/auth/register/finish"))
            .json(&json!({ "session_id": begin["session_id"], "credential": credential }))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 204);
        Ok(session_cookie(&resp))
    };
    // Lost, or expired: the machine makes a new one, with no restart, and the
    // one the server printed stops working.
    let (ok, out, err) = seedling(&server.db_path, &["admin", "link"]);
    assert!(ok, "{out}{err}");
    assert!(err.contains("No account yet"), "{err}");
    let fresh = address_in(&out, "/setup/#link=").expect("a setup address");
    let printed = link;
    let link = fresh.split("link=").nth(1).unwrap().to_owned();
    assert_ne!(link, printed);
    let refused = sign_up(
        &mut WebauthnAuthenticator::new(SoftPasskey::new(true)),
        &printed,
    )
    .await
    .unwrap_err();
    assert_eq!(refused["code"], "link_invalid");

    let cookie = sign_up(&mut passkey, &link)
        .await
        .expect("the first account");
    let session = json_body(
        server
            .client
            .get(format!("{base}/auth/session"))
            .header(reqwest::header::COOKIE, &cookie)
            .send()
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(session["admin"], true);
    let refused = sign_up(
        &mut WebauthnAuthenticator::new(SoftPasskey::new(true)),
        &link,
    )
    .await
    .unwrap_err();
    assert_eq!(refused["code"], "link_invalid");

    // The administrator reads the accounts, and nothing of anyone's
    // identities is there to read.
    let accounts = json_body(
        server
            .client
            .get(format!("{base}/admin/accounts"))
            .header(reqwest::header::COOKIE, &cookie)
            .send()
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(accounts.as_array().unwrap().len(), 1);
    assert_eq!(accounts[0]["admin"], true);
    assert!(accounts[0].get("identities").is_none());

    // From the machine: who may sign up, read and set.
    let (ok, out, _) = seedling(&server.db_path, &["signup"]);
    assert!(ok && out.trim() == "invitation", "{out}");
    let (ok, out, _) = seedling(&server.db_path, &["signup", "closed"]);
    assert!(ok && out.trim() == "closed", "{out}");
    let (ok, _, err) = seedling(&server.db_path, &["signup", "whenever"]);
    assert!(!ok && err.contains("open, invitation or closed"), "{err}");
    let info = json_body(
        server
            .client
            .get(format!("{base}/auth/server"))
            .send()
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(info["policy"], "closed");
    // It is in the log, as done on the machine.
    let log = json_body(
        server
            .client
            .get(format!("{base}/admin/log"))
            .header(reqwest::header::COOKIE, &cookie)
            .send()
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(log[0]["action"], "signup.policy");
    assert_eq!(log[0]["by"], "machine");
    assert_eq!(log[0]["detail"], "closed");
}

#[tokio::test]
async fn a_server_with_accounts_and_no_administrator_gets_one_from_the_machine() {
    // As a server upgraded from a version without administration.
    let server = start_seedling();
    let base = &server.base_url;
    let origin = url::Url::parse(base).unwrap();
    let mut passkey = WebauthnAuthenticator::new(SoftPasskey::new(true));
    let resp = server
        .client
        .post(format!("{base}/auth/register/begin"))
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let begin = json_body(resp).await;
    let ccr: CreationChallengeResponse =
        serde_json::from_value(begin["challenge"].clone()).unwrap();
    let credential = passkey
        .do_registration(origin, for_soft_passkey(ccr))
        .expect("registration ceremony");
    let resp = server
        .client
        .post(format!("{base}/auth/register/finish"))
        .json(&json!({ "session_id": begin["session_id"], "credential": credential }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 204);
    let cookie = session_cookie(&resp);
    let resp = server
        .client
        .post(format!("{base}/account/recovery-code"))
        .header(reqwest::header::COOKIE, &cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // The operator makes a link; the signed-in account opens it.
    let (ok, out, err) = seedling(&server.db_path, &["admin", "link"]);
    assert!(ok, "{out}{err}");
    let address = address_in(&out, "/admin/claim/#link=").expect("an address is printed");
    assert!(address.starts_with("http://localhost:"), "{address}");
    let link = address.split("link=").nth(1).unwrap().to_owned();
    let claim = async || {
        server
            .client
            .post(format!("{base}/admin/claim"))
            .header(reqwest::header::COOKIE, &cookie)
            .json(&json!({ "link": link }))
            .send()
            .await
            .unwrap()
            .status()
    };
    assert_eq!(claim().await, 204);
    assert_eq!(claim().await, 403, "once");
    let session = json_body(
        server
            .client
            .get(format!("{base}/auth/session"))
            .header(reqwest::header::COOKIE, &cookie)
            .send()
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(session["admin"], true);
}
