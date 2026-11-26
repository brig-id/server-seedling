//! The installation guide (`docs/install.md`) cannot drift from the program:
//! every `seedling` command it shows is given to the real argument parser,
//! which refuses a command or an option that does not exist; and every code
//! block in it is either run or excused, with a reason.
//!
//! The guide job of CI (`scripts/run-guide.sh`) follows it on a fresh
//! machine: it writes the guide's blocks as one shell script ([`script`],
//! through `write_the_guide_as_a_script`), runs it, then signs a person in on
//! the site the guide registered
//! (`a_person_signs_in_on_the_site_the_guide_registered`).

// Each test crate includes this module and uses part of it.
#[allow(dead_code)]
mod common;

use std::process::Command;

use common::{answer_discoverable, for_soft_passkey};
use serde_json::{Value, json};
use webauthn_authenticator_rs::{WebauthnAuthenticator, softpasskey::SoftPasskey};
use webauthn_rs::prelude::{CreationChallengeResponse, RequestChallengeResponse};

const GUIDE: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/docs/install.md"));

/// A fenced code block of the guide.
struct Block {
    /// The line of its opening fence, counted from 1.
    line: usize,
    /// What follows the opening fence: `sh` for commands.
    lang: String,
    /// What the HTML comment on the line before the fence says, if there is
    /// one: `run`, `file: <path>` or `not run: <why>`.
    marker: Option<String>,
    /// Its lines, each with its end of line.
    body: String,
}

/// What becomes of a block when the guide is followed.
#[derive(Debug, PartialEq)]
enum Fate<'a> {
    /// Commands, run in the guide's order.
    Run,
    /// A file the reader writes at this path.
    File(&'a str),
    /// Not run, for this reason.
    NotRun(&'a str),
}

impl Block {
    fn fate(&self) -> Option<Fate<'_>> {
        let marker = self.marker.as_deref()?;
        if marker == "run" {
            Some(Fate::Run)
        } else if let Some(path) = marker.strip_prefix("file: ") {
            path.starts_with('/').then_some(Fate::File(path))
        } else {
            marker
                .strip_prefix("not run: ")
                .filter(|why| !why.trim().is_empty())
                .map(Fate::NotRun)
        }
    }
}

/// The fenced blocks of `guide`, in order.
fn blocks(guide: &str) -> Vec<Block> {
    let mut found = Vec::new();
    let mut open: Option<Block> = None;
    let mut previous = "";
    for (index, line) in guide.lines().enumerate() {
        if let Some(fence) = line.strip_prefix("```") {
            match open.take() {
                Some(block) => found.push(block),
                None => {
                    open = Some(Block {
                        line: index + 1,
                        lang: fence.trim().to_owned(),
                        marker: previous
                            .trim()
                            .strip_prefix("<!--")
                            .and_then(|m| m.strip_suffix("-->"))
                            .map(|m| m.trim().to_owned()),
                        body: String::new(),
                    });
                }
            }
        } else if let Some(block) = open.as_mut() {
            block.body.push_str(line);
            block.body.push('\n');
        }
        previous = line;
    }
    assert!(open.is_none(), "a code block of the guide is never closed");
    found
}

/// The guide as one shell script: its `run` blocks and its files, in order.
fn script(guide: &str) -> String {
    let mut script = String::from(
        "#!/usr/bin/env bash\n\
         # docs/install.md, followed: written by tests/install_guide.rs.\n\
         set -euxo pipefail\n",
    );
    for block in blocks(guide) {
        match block.fate() {
            Some(Fate::Run) => {
                script.push_str(&format!("\n# docs/install.md:{}\n", block.line));
                script.push_str(&block.body);
            }
            Some(Fate::File(path)) => {
                script.push_str(&format!(
                    "\n# docs/install.md:{}\nsudo tee {path} > /dev/null <<'GUIDE_FILE'\n{}GUIDE_FILE\n",
                    block.line, block.body
                ));
            }
            Some(Fate::NotRun(_)) | None => {}
        }
    }
    script
}

/// The words of a command line as a shell splits them: spaces separate,
/// double quotes and `$(…)` keep a phrase together, `#` starts a comment.
fn words(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quoted = false;
    let mut started = false;
    // How deep in `$(…)`.
    let mut substituted = 0usize;
    for c in line.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                started = true;
            }
            '#' if !quoted && !started => break,
            '(' if word.ends_with('$') => {
                substituted += 1;
                word.push(c);
            }
            ')' if substituted > 0 => {
                substituted -= 1;
                word.push(c);
            }
            c if c.is_whitespace() && !quoted && substituted == 0 => {
                if started {
                    words.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            c => {
                word.push(c);
                started = true;
            }
        }
    }
    if started {
        words.push(word);
    }
    words
}

/// The `seedling` commands of `blocks`, without `sudo`.
fn commands<'a>(blocks: impl IntoIterator<Item = &'a Block>) -> Vec<Vec<String>> {
    blocks
        .into_iter()
        .filter(|block| block.lang == "sh")
        .flat_map(|block| block.body.lines().map(words))
        .filter_map(|mut w| {
            if w.first().is_some_and(|w| w == "sudo") {
                w.remove(0);
            }
            w.first().is_some_and(|w| w == "seedling").then_some(w)
        })
        .collect()
}

/// The word after `option` in the first `seedling <start…>` command that
/// the guide runs.
fn option_of(start: &[&str], option: &str) -> String {
    let blocks = blocks(GUIDE);
    let run = blocks
        .iter()
        .filter(|block| block.fate() == Some(Fate::Run));
    let command = commands(run)
        .into_iter()
        .find(|c| c.len() > start.len() && c[1..=start.len()] == *start)
        .unwrap_or_else(|| panic!("the guide runs no `seedling {}`", start.join(" ")));
    let at = command
        .iter()
        .position(|w| w == option)
        .unwrap_or_else(|| panic!("`seedling {}` has no {option}", start.join(" ")));
    command[at + 1].clone()
}

#[test]
fn the_shell_is_split_as_a_shell_would() {
    assert_eq!(
        words(r#"seedling client add --name "Field Notes" --public  # a comment"#),
        [
            "seedling",
            "client",
            "add",
            "--name",
            "Field Notes",
            "--public"
        ]
    );
    assert_eq!(words("seedling --version"), ["seedling", "--version"]);
    assert_eq!(
        words("seedling backup /b/brigid-$(date -u +%F).db"),
        ["seedling", "backup", "/b/brigid-$(date -u +%F).db"]
    );
}

#[test]
fn every_command_of_the_guide_is_one_seedling_knows() {
    let commands = commands(&blocks(GUIDE));
    // The guide's steps: preparing, opening, registering, and the day to day.
    for expected in ["init", "signup", "client", "admin", "--version"] {
        assert!(
            commands
                .iter()
                .any(|c| c.get(1).is_some_and(|w| w == expected)),
            "the guide shows no `seedling {expected}`"
        );
    }
    for command in &commands {
        // `--help` after the rest: the parser reads every word before it,
        // refuses one it does not know, then prints help and does nothing.
        let mut args: Vec<&str> = command[1..].iter().map(String::as_str).collect();
        if args != ["--version"] {
            args.push("--help");
        }
        let out = Command::new(env!("CARGO_BIN_EXE_seedling"))
            .args(&args)
            .env_clear()
            .output()
            .expect("run seedling");
        assert!(
            out.status.success(),
            "`{}` is not a command seedling knows:\n{}",
            command.join(" "),
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

/// Every code block says, on the line before it, what becomes of it when
/// the guide is followed: commands are run, or excused with a reason; a
/// file is written where it says, or excused too.
#[test]
fn every_block_of_the_guide_is_run_or_excused() {
    let blocks = blocks(GUIDE);
    assert!(blocks.len() > 10, "{} blocks", blocks.len());
    for block in &blocks {
        let fits = match (block.fate(), block.lang.as_str()) {
            (Some(Fate::Run), lang) => lang == "sh",
            (Some(Fate::File(_)), lang) => lang != "sh",
            (Some(Fate::NotRun(_)), _) => true,
            (None, _) => false,
        };
        assert!(
            fits,
            "docs/install.md:{}: a `{}` block needs, on the line before it, \
             `<!-- run -->` (for `sh`), `<!-- file: /its/path -->` (for a file) or \
             `<!-- not run: <why> -->`; it has {:?}",
            block.line, block.lang, block.marker
        );
    }
    // What the guide is for is run, not excused.
    let script = script(GUIDE);
    for step in [
        "| (cd \"$TMP\" && sha256sum --check)",
        "sudo seedling init --domain ",
        "sudo systemctl enable --now seedling",
        "curl -fsS https://",
        "sudo seedling admin link",
        "sudo seedling signup open",
        "sudo seedling client add ",
        "sudo systemctl restart seedling",
        "sudo seedling backup ",
        "sudo systemctl stop seedling\nsudo seedling restore ",
        "sudo seedling limits set ",
    ] {
        assert!(script.contains(step), "the guide does not run `{step}`");
    }
}

#[test]
fn the_script_holds_the_run_blocks_and_the_files_in_order() {
    let guide = "\
<!-- run -->
```sh
echo one
```

<!-- not run: an example -->
```sh
echo never
```

<!-- file: /etc/x.conf -->
```
key = 1
```

<!-- run -->
```sh
cat /etc/x.conf
```
";
    let script = script(guide);
    assert!(!script.contains("never"), "{script}");
    let one = script.find("echo one").unwrap();
    let file = script
        .find("sudo tee /etc/x.conf > /dev/null <<'GUIDE_FILE'\nkey = 1\nGUIDE_FILE\n")
        .unwrap();
    let cat = script.find("cat /etc/x.conf").unwrap();
    assert!(one < file && file < cat, "{script}");
    assert!(script.contains("# docs/install.md:2\n"), "{script}");
}

/// Step 3 of the guide is what `seedling init --systemd` prints: the same
/// four commands, word for word.
#[test]
fn the_guide_starts_the_server_as_init_says() {
    let step = GUIDE
        .split("\n## 3. ")
        .nth(1)
        .expect("a step 3")
        .split("\n## ")
        .next()
        .unwrap();
    let block = step
        .split("```sh\n")
        .nth(1)
        .expect("a shell block in step 3")
        .split("```")
        .next()
        .unwrap();
    assert_eq!(block, seedling::machine::START_COMMANDS);
}

/// Writes [`script`] of the guide at `$GUIDE_SCRIPT`, for the guide job of
/// CI.
#[test]
#[ignore = "writes the script the guide job of CI runs"]
fn write_the_guide_as_a_script() {
    let path = std::env::var("GUIDE_SCRIPT").expect("GUIDE_SCRIPT: where to write the script");
    std::fs::write(&path, script(GUIDE)).expect("write the script");
}

/// Steps 5 to 8 on the server the guide installed: the first administrator
/// from the setup link, a sign-in with the passkey, then the site the guide
/// registered signs that person in. A software passkey and HTTP requests
/// stand for the person and the browser.
///
/// Run by the guide job of CI, after the guide, with what the machine
/// printed: `GUIDE_SETUP_ADDRESS` (`seedling admin link`),
/// `GUIDE_CLIENT_ID` and `GUIDE_CLIENT_SECRET` (`seedling client add`).
/// The server is reached at `https://<the guide's domain>`, or at
/// `GUIDE_SERVER`.
#[tokio::test]
#[ignore = "needs the server the guide installed (the guide job of CI)"]
async fn a_person_signs_in_on_the_site_the_guide_registered() {
    use base64ct::{Base64UrlUnpadded, Encoding};

    // RFC 7636 appendix B.
    const VERIFIER: &str = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
    const CHALLENGE: &str = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";

    let var = |name: &str| std::env::var(name).unwrap_or_else(|_| panic!("{name} is not set"));
    let issuer = format!("https://{}", option_of(&["init"], "--domain"));
    let base = std::env::var("GUIDE_SERVER").unwrap_or_else(|_| issuer.clone());
    let redirect = option_of(&["client", "add"], "--redirect-uri");
    let site = option_of(&["client", "add"], "--name");
    let address = var("GUIDE_SETUP_ADDRESS");
    let client_id = var("GUIDE_CLIENT_ID");
    let secret = var("GUIDE_CLIENT_SECRET");

    async fn json(resp: reqwest::Response) -> Value {
        resp.json().await.expect("a JSON body")
    }
    // The interface, as a browser runs it on the server's page: every request
    // carries that origin.
    let page = reqwest::Client::builder()
        .default_headers(
            [(
                reqwest::header::ORIGIN,
                reqwest::header::HeaderValue::from_str(&issuer).unwrap(),
            )]
            .into_iter()
            .collect(),
        )
        .build()
        .unwrap();
    let origin = url::Url::parse(&issuer).unwrap();
    let mut passkey = WebauthnAuthenticator::new(SoftPasskey::new(true));

    // -- step 5: the first administrator, from the link the machine printed --
    let link = address
        .strip_prefix(&format!("{issuer}/setup/#link="))
        .unwrap_or_else(|| panic!("not a setup address of {issuer}: {address}"));
    let resp = page
        .post(format!("{base}/auth/register/begin"))
        .json(&json!({ "link": link }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "the setup link opens the sign-up");
    let begin = json(resp).await;
    let ccr: CreationChallengeResponse =
        serde_json::from_value(begin["challenge"].clone()).unwrap();
    let account_id = ccr.public_key.user.id.to_vec();
    let credential = passkey
        .do_registration(origin.clone(), for_soft_passkey(ccr))
        .expect("registration ceremony");
    let resp = page
        .post(format!("{base}/auth/register/finish"))
        .json(&json!({ "session_id": begin["session_id"], "credential": credential }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 204);

    // -- the person comes back, and signs in with the passkey --
    let begin = json(
        page.post(format!("{base}/auth/login/begin"))
            .json(&json!({}))
            .send()
            .await
            .unwrap(),
    )
    .await;
    let rcr: RequestChallengeResponse = serde_json::from_value(begin["challenge"].clone()).unwrap();
    let assertion = answer_discoverable(&mut passkey, &origin, rcr, &credential.id, &account_id);
    let resp = page
        .post(format!("{base}/auth/login/finish"))
        .json(&json!({ "session_id": begin["session_id"], "credential": assertion }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 204, "the passkey signs in");
    let cookie = resp
        .headers()
        .get(reqwest::header::SET_COOKIE)
        .expect("a session")
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let session = json(
        page.get(format!("{base}/auth/session"))
            .header(reqwest::header::COOKIE, &cookie)
            .send()
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(session["admin"], true, "the first account administers");

    // -- the onboarding: the recovery code, then an identity --
    let resp = page
        .post(format!("{base}/account/recovery-code"))
        .header(reqwest::header::COOKIE, &cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let created = json(
        page.post(format!("{base}/account/identities"))
            .header(reqwest::header::COOKIE, &cookie)
            .json(&json!({ "name": "Personal", "logo": "icon:ph:user" }))
            .send()
            .await
            .unwrap(),
    )
    .await;
    let identity = created["id"].as_str().expect("an identity").to_owned();
    let resp = page
        .patch(format!("{base}/account/identities/{identity}"))
        .header(reqwest::header::COOKIE, &cookie)
        .json(&json!({ "attributes": { "email": "ada@example.org", "given_name": "Ada" } }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // -- steps 7 and 8: the site sends the browser there, and is sent back --
    let discovery = json(
        page.get(format!("{base}/.well-known/openid-configuration"))
            .send()
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(discovery["issuer"], *issuer);
    let browser = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let authorize = url::Url::parse_with_params(
        &format!("{base}/authorize"),
        [
            ("response_type", "code"),
            ("scope", "openid profile email"),
            ("client_id", &client_id),
            ("redirect_uri", &redirect),
            ("state", "xyz"),
            ("nonce", "n-0S6"),
            ("code_challenge", CHALLENGE),
            ("code_challenge_method", "S256"),
        ],
    )
    .unwrap();
    let resp = browser.get(authorize).send().await.unwrap();
    assert_eq!(resp.status(), 303);
    let location = resp.headers()["location"].to_str().unwrap().to_owned();
    let request = location
        .strip_prefix("/continue/?request=")
        .unwrap_or_else(|| panic!("unexpected redirect: {location}"));
    let view = json(
        page.get(format!("{base}/auth/authorize/{request}"))
            .header(reqwest::header::COOKIE, &cookie)
            .send()
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(view["client"]["name"], *site, "{view}");
    let resp = page
        .post(format!("{base}/auth/authorize/{request}/approve"))
        .header(reqwest::header::COOKIE, &cookie)
        .json(&json!({ "identity_id": identity, "attributes": ["email", "given_name"] }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let answer = json(resp).await;
    let back = url::Url::parse(answer["redirect_to"].as_str().unwrap()).unwrap();
    assert!(back.as_str().starts_with(&redirect), "{back}");
    let query: std::collections::HashMap<_, _> = back.query_pairs().into_owned().collect();
    assert_eq!(query["state"], "xyz");
    assert_eq!(query["iss"], *issuer);

    // -- the site exchanges the code, with its secret and the PKCE verifier --
    let form = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs([
            ("grant_type", "authorization_code"),
            ("code", query["code"].as_str()),
            ("redirect_uri", redirect.as_str()),
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
    let tokens = json(resp).await;
    let payload = tokens["id_token"]
        .as_str()
        .expect("an ID token")
        .split('.')
        .nth(1)
        .expect("a JWT");
    let claims: Value =
        serde_json::from_slice(&Base64UrlUnpadded::decode_vec(payload).unwrap()).unwrap();
    assert_eq!(claims["iss"], *issuer);
    assert_eq!(claims["aud"], client_id);
    assert_eq!(claims["nonce"], "n-0S6");
    assert_eq!(claims["email"], "ada@example.org", "{claims}");
    assert_eq!(claims["given_name"], "Ada", "{claims}");
    println!("✓ {site} signed a person in through {issuer}");
}
