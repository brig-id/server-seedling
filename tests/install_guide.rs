//! The installation guide (`docs/install.md`) cannot drift from the program:
//! every `seedling` command it shows is given to the real argument parser,
//! which refuses a command or an option that does not exist.

use std::{fs, process::Command};

/// The words of a command line as a shell splits them: spaces separate,
/// double quotes keep a phrase together, `#` starts a comment.
fn words(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quoted = false;
    let mut started = false;
    for c in line.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                started = true;
            }
            '#' if !quoted && !started => break,
            c if c.is_whitespace() && !quoted => {
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

/// The `seedling` commands of the guide's shell blocks, without `sudo`.
fn commands(guide: &str) -> Vec<Vec<String>> {
    let mut found = Vec::new();
    let mut in_shell = false;
    for line in guide.lines() {
        if let Some(fence) = line.strip_prefix("```") {
            in_shell = !in_shell && fence.trim() == "sh";
            continue;
        }
        if !in_shell {
            continue;
        }
        let mut w = words(line);
        if w.first().is_some_and(|w| w == "sudo") {
            w.remove(0);
        }
        if w.first().is_some_and(|w| w == "seedling") {
            found.push(w);
        }
    }
    found
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
}

#[test]
fn every_command_of_the_guide_is_one_seedling_knows() {
    let guide = fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/docs/install.md"))
        .expect("docs/install.md");
    let commands = commands(&guide);
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

/// Step 3 of the guide is what `seedling init --systemd` prints: the same
/// four commands, word for word.
#[test]
fn the_guide_starts_the_server_as_init_says() {
    let guide = fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/docs/install.md"))
        .expect("docs/install.md");
    let step = guide
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
