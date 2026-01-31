//! The fixed list of `gh` and `glab` command forms an agent may call, by the `github.gh` and
//! `gitlab.glab` tools (ProjectService.md SA-RECON-10) and in a shell command (K.3): a form off the
//! list refuses the tool and sends the shell command to a card.
//!
//! Anything else is refused before a program starts: `api`, `alias`, `extension`, `auth` and
//! `config` have no form, so no call reaches the token or code of the person's choosing. A flag is
//! on a form's list or it is refused, because `--repo`, `--hostname`, `--web`, `--editor` and
//! `--body-file` each reach something the form does not show: another repository, another host, a
//! program, or a local file. `--jq` and `-q` read the environment: run on gh 2.102.0,
//! `--jq env.GH_TOKEN` printed the token. `--template` is a second language over the output with no
//! form needing it.

/// The two programs the list covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostProgram {
    Gh,
    Glab,
}

impl HostProgram {
    fn name(self) -> &'static str {
        match self {
            HostProgram::Gh => "gh",
            HostProgram::Glab => "glab",
        }
    }
}

/// What a form does to the host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    /// Runs on the MCP Front's decision alone.
    Read,
    /// Needs the Rules Engine's decision on the command (K.3).
    Write,
}

struct Form {
    command: [&'static str; 2],
    effect: Effect,
    /// How many words the form takes besides its flags.
    positionals: usize,
    switches: &'static [&'static str],
    values: &'static [&'static str],
}

const fn read(
    command: [&'static str; 2],
    positionals: usize,
    switches: &'static [&'static str],
    values: &'static [&'static str],
) -> Form {
    Form {
        command,
        effect: Effect::Read,
        positionals,
        switches,
        values,
    }
}

const fn write(
    command: [&'static str; 2],
    positionals: usize,
    switches: &'static [&'static str],
    values: &'static [&'static str],
) -> Form {
    Form {
        command,
        effect: Effect::Write,
        positionals,
        switches,
        values,
    }
}

const GH_LISTING: &[&str] = &[
    "--state",
    "-s",
    "--limit",
    "-L",
    "--author",
    "-A",
    "--assignee",
    "-a",
    "--label",
    "-l",
    "--search",
    "-S",
    "--json",
];

const GH_PULL_LISTING: &[&str] = &[
    "--state",
    "-s",
    "--limit",
    "-L",
    "--author",
    "-A",
    "--assignee",
    "-a",
    "--label",
    "-l",
    "--search",
    "-S",
    "--json",
    "--base",
    "-B",
    "--head",
    "-H",
];

// spec: ordin-desktop/components/ProjectService#SA-RECON-10
const GH_FORMS: &[Form] = &[
    read(["pr", "view"], 1, &["--comments", "-c"], &["--json"]),
    read(["pr", "list"], 0, &["--draft", "-d"], GH_PULL_LISTING),
    read(["pr", "status"], 0, &[], &["--json"]),
    read(["pr", "diff"], 1, &["--name-only", "--patch"], &[]),
    read(["pr", "checks"], 1, &["--required"], &["--json"]),
    read(["issue", "view"], 1, &["--comments", "-c"], &["--json"]),
    read(["issue", "list"], 0, &[], GH_LISTING),
    read(["issue", "status"], 0, &[], &["--json"]),
    read(["repo", "view"], 0, &[], &["--json"]),
    read(["release", "view"], 1, &[], &["--json"]),
    read(["release", "list"], 0, &[], &["--limit", "-L", "--json"]),
    read(
        ["run", "view"],
        1,
        &["--log", "--log-failed", "--verbose", "-v"],
        &["--job", "-j", "--attempt", "-a", "--json"],
    ),
    read(
        ["run", "list"],
        0,
        &[],
        &[
            "--limit",
            "-L",
            "--branch",
            "-b",
            "--status",
            "-s",
            "--workflow",
            "-w",
            "--event",
            "-e",
            "--json",
        ],
    ),
    write(
        ["pr", "create"],
        0,
        &["--draft", "-d", "--fill"],
        &[
            "--title",
            "-t",
            "--body",
            "-b",
            "--base",
            "-B",
            "--head",
            "-H",
            "--reviewer",
            "-r",
            "--assignee",
            "-a",
            "--label",
            "-l",
            "--milestone",
            "-m",
        ],
    ),
    write(
        ["pr", "edit"],
        1,
        &[],
        &[
            "--title",
            "-t",
            "--body",
            "-b",
            "--base",
            "-B",
            "--add-reviewer",
            "--remove-reviewer",
            "--add-label",
            "--remove-label",
            "--add-assignee",
            "--remove-assignee",
            "--milestone",
            "-m",
        ],
    ),
    write(["pr", "comment"], 1, &[], &["--body", "-b"]),
    write(["issue", "comment"], 1, &[], &["--body", "-b"]),
    write(
        ["pr", "review"],
        1,
        &[
            "--approve",
            "-a",
            "--comment",
            "-c",
            "--request-changes",
            "-r",
        ],
        &["--body", "-b"],
    ),
    write(
        ["pr", "merge"],
        1,
        &[
            "--merge",
            "-m",
            "--squash",
            "-s",
            "--rebase",
            "-r",
            "--delete-branch",
            "-d",
            "--auto",
        ],
        &["--subject", "-t", "--body", "-b"],
    ),
    write(
        ["pr", "close"],
        1,
        &["--delete-branch", "-d"],
        &["--comment", "-c"],
    ),
    write(
        ["issue", "close"],
        1,
        &[],
        &["--comment", "-c", "--reason", "-r"],
    ),
    write(
        ["release", "create"],
        1,
        &["--draft", "-d", "--prerelease", "-p"],
        &["--title", "-t", "--notes", "-n", "--target"],
    ),
    write(
        ["label", "create"],
        1,
        &[],
        &["--color", "-c", "--description", "-d"],
    ),
    write(
        ["label", "edit"],
        1,
        &[],
        &["--name", "-n", "--color", "-c", "--description", "-d"],
    ),
];

const GLAB_FORMS: &[Form] = &[
    read(["mr", "view"], 1, &["--comments", "-c"], &["--output"]),
    read(
        ["mr", "list"],
        0,
        &[
            "--all", "-A", "--closed", "-c", "--merged", "-M", "--draft", "-d",
        ],
        &[
            "--author",
            "--assignee",
            "-a",
            "--label",
            "-l",
            "--source-branch",
            "-s",
            "--target-branch",
            "-t",
            "--search",
            "--per-page",
            "-P",
            "--page",
            "-p",
            "--output",
        ],
    ),
    read(["mr", "diff"], 1, &["--raw"], &[]),
    read(["issue", "view"], 1, &["--comments", "-c"], &["--output"]),
    read(
        ["issue", "list"],
        0,
        &["--all", "-A", "--closed", "-c"],
        &[
            "--author",
            "--assignee",
            "-a",
            "--label",
            "-l",
            "--search",
            "--per-page",
            "-P",
            "--page",
            "-p",
            "--output",
        ],
    ),
    read(["release", "view"], 1, &[], &[]),
    read(
        ["release", "list"],
        0,
        &[],
        &["--per-page", "-P", "--page", "-p"],
    ),
    write(
        ["mr", "create"],
        0,
        &[
            "--draft",
            "--fill",
            "--yes",
            "-y",
            "--remove-source-branch",
            "--squash-before-merge",
        ],
        &[
            "--title",
            "-t",
            "--description",
            "-d",
            "--source-branch",
            "-s",
            "--target-branch",
            "-b",
            "--label",
            "-l",
            "--assignee",
            "-a",
            "--reviewer",
            "--milestone",
            "-m",
        ],
    ),
    write(
        ["mr", "update"],
        1,
        &["--draft", "--ready", "-r", "--remove-source-branch"],
        &[
            "--title",
            "-t",
            "--description",
            "-d",
            "--label",
            "-l",
            "--unlabel",
            "-u",
            "--assignee",
            "-a",
            "--reviewer",
            "--target-branch",
        ],
    ),
    write(["mr", "note"], 1, &[], &["--message", "-m"]),
    write(["issue", "note"], 1, &[], &["--message", "-m"]),
    write(
        ["mr", "merge"],
        1,
        &[
            "--squash",
            "-s",
            "--rebase",
            "-r",
            "--remove-source-branch",
            "-d",
            "--yes",
            "-y",
        ],
        &["--message", "-m", "--squash-message"],
    ),
    write(["mr", "close"], 1, &[], &[]),
    write(["issue", "close"], 1, &[], &[]),
    write(
        ["release", "create"],
        1,
        &[],
        &["--name", "-n", "--notes", "-N", "--ref", "-r"],
    ),
    write(
        ["label", "create"],
        0,
        &[],
        &["--name", "-n", "--color", "-c", "--description", "-d"],
    ),
];

/// A number, branch or tag of the repository in the folder. A `:` would make a word an address, so
/// a form could not reach another repository through a URL or `owner/repo#1`.
fn positional_ok(word: &str) -> bool {
    !word.is_empty()
        && !word.starts_with(['-', '/'])
        && !word.contains("..")
        && !word.contains("//")
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._/-".contains(c))
}

/// A search qualifier naming where to look, which could reach beyond the repository in the folder.
fn widens_search(value: &str) -> bool {
    let value = value.to_lowercase();
    ["repo:", "org:", "user:", "owner:"]
        .iter()
        .any(|qualifier| value.contains(qualifier))
}

/// The form of `arguments`, or the sentence that refuses them. Nothing here reads the host.
pub fn check(kind: HostProgram, arguments: &[String]) -> Result<Effect, String> {
    let forms = match kind {
        HostProgram::Gh => GH_FORMS,
        HostProgram::Glab => GLAB_FORMS,
    };
    let tool = kind.name();
    let (Some(group), Some(verb)) = (arguments.first(), arguments.get(1)) else {
        return Err(format!(
            "{tool} takes a fixed list of commands, and this is not one."
        ));
    };
    let form = forms
        .iter()
        .find(|form| form.command[0] == group && form.command[1] == verb)
        .ok_or_else(|| format!("{tool} takes a fixed list of commands, and this is not one."))?;
    let mut positionals = 0;
    let mut words = arguments[2..].iter();
    while let Some(word) = words.next() {
        if word.starts_with('-') {
            let (name, inline) = match word.split_once('=') {
                Some((name, value)) => (name, Some(value)),
                None => (word.as_str(), None),
            };
            if form.values.contains(&name) {
                let Some(value) = inline.or_else(|| words.next().map(String::as_str)) else {
                    return Err(format!("{name} needs a value."));
                };
                if matches!(name, "--search" | "-S") && widens_search(value) {
                    return Err("A search stays in this repository.".to_owned());
                }
            } else if !(inline.is_none() && form.switches.contains(&name)) {
                return Err(format!("{tool} {group} {verb} does not take {name} here."));
            }
        } else if positional_ok(word) && positionals < form.positionals {
            positionals += 1;
        } else {
            return Err(format!(
                "{tool} {group} {verb} takes only a number, branch or tag of this repository."
            ));
        }
    }
    Ok(form.effect)
}

/// The command as the Rules Engine reads it (K.3): one word for each argument.
pub fn command_text(kind: HostProgram, arguments: &[String]) -> String {
    let mut text = kind.name().to_owned();
    for argument in arguments {
        text.push(' ');
        let plain = !argument.is_empty()
            && argument
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "_-./=:@,".contains(c));
        if plain {
            text.push_str(argument);
        } else {
            text.push('\'');
            text.push_str(&argument.replace('\'', r"'\''"));
            text.push('\'');
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(words: &[&str]) -> Vec<String> {
        words.iter().map(|word| (*word).to_owned()).collect()
    }

    #[test]
    fn the_listed_forms_pass_with_their_effect() {
        for (kind, words, effect) in [
            (HostProgram::Gh, &["pr", "list"][..], Effect::Read),
            (
                HostProgram::Gh,
                &["pr", "view", "12", "--json", "title"],
                Effect::Read,
            ),
            (HostProgram::Gh, &["pr", "view", "ordin/x"], Effect::Read),
            (
                HostProgram::Gh,
                &["pr", "comment", "12", "--body", "--repo x"],
                Effect::Write,
            ),
            (
                HostProgram::Gh,
                &["pr", "comment", "12", "--body=done"],
                Effect::Write,
            ),
            (
                HostProgram::Gh,
                &["pr", "merge", "12", "--squash"],
                Effect::Write,
            ),
            (
                HostProgram::Glab,
                &["mr", "note", "3", "-m", "done"],
                Effect::Write,
            ),
            (HostProgram::Glab, &["mr", "view", "3"], Effect::Read),
        ] {
            assert_eq!(check(kind, &args(words)), Ok(effect), "{words:?}");
        }
    }

    #[test]
    fn a_search_stays_in_this_repository() {
        for value in ["repo:other/secret is:open", "ORG:x", "a user:y"] {
            let words = args(&["pr", "list", "--search", value]);
            assert!(check(HostProgram::Gh, &words).is_err(), "{value}");
        }
        let plain = args(&["pr", "list", "--search", "is:open"]);
        assert_eq!(check(HostProgram::Gh, &plain), Ok(Effect::Read));
    }

    #[test]
    fn anything_off_the_list_is_refused() {
        for words in [
            &["api", "/user"][..],
            &["api"],
            &["alias", "set", "x", "--shell", "sh"],
            &["extension", "install", "a/b"],
            &["auth", "token"],
            &["auth", "status"],
            &["config", "get", "x"],
            &["repo", "delete", "a/b"],
            &["repo", "clone", "a/b"],
            &["secret", "list"],
            &["pr", "checkout", "1"],
            &["pr"],
            &["pr", "list", "--repo", "other/repo"],
            &["pr", "list", "-R", "other/repo"],
            &["pr", "list", "-Rother/repo"],
            &["pr", "list", "--repo=other/repo"],
            &["pr", "list", "--hostname", "evil.example"],
            &["pr", "view", "1", "--web"],
            &["pr", "view", "1", "--jq", "env.GH_TOKEN"],
            &["pr", "view", "1", "-q", "env.GH_TOKEN"],
            &["pr", "view", "1", "--template", "{{.}}"],
            &["pr", "view", "https://github.com/other/repo/pull/1"],
            &["pr", "view", "other/repo#1"],
            &["pr", "view", "1", "2"],
            &["pr", "view", "../x"],
            &["pr", "view", "--help"],
            &["pr", "view", "--"],
            &["pr", "comment", "1", "--body-file", "/etc/passwd"],
            &["pr", "comment", "1", "-F", "/etc/passwd"],
            &["pr", "comment", "1", "--editor"],
            &["pr", "comment", "1", "--body"],
            &["pr", "merge", "1", "--admin"],
            &["pr", "merge", "1", "--squash=true"],
            &["release", "create", "v1", "./secret.key"],
            &["release", "create", "v1", "--notes-file", "x"],
        ] {
            assert!(check(HostProgram::Gh, &args(words)).is_err(), "{words:?}");
        }
        for words in [
            &["api", "/user"][..],
            &["mr", "list", "--repo", "other/repo"],
            &["mr", "list", "-R", "other/repo"],
            &["mr", "view", "https://gitlab.com/a/b/-/merge_requests/1"],
            &["mr", "view", "1", "--web"],
            &["variable", "list"],
            &["release", "create", "v1", "./secret.key"],
        ] {
            assert!(check(HostProgram::Glab, &args(words)).is_err(), "{words:?}");
        }
    }

    #[test]
    fn a_flag_is_on_one_list_only_and_every_form_is_unique() {
        for forms in [GH_FORMS, GLAB_FORMS] {
            for (at, form) in forms.iter().enumerate() {
                assert!(form.switches.iter().all(|flag| !form.values.contains(flag)));
                assert!(forms[at + 1..]
                    .iter()
                    .all(|other| other.command != form.command));
            }
        }
    }

    #[test]
    fn the_command_text_keeps_each_argument_one_word() {
        assert_eq!(
            command_text(
                HostProgram::Gh,
                &args(&["pr", "comment", "12", "--body", "done"])
            ),
            "gh pr comment 12 --body done"
        );
        assert_eq!(
            command_text(
                HostProgram::Gh,
                &args(&["pr", "comment", "1", "--body", "it's; rm x"])
            ),
            r"gh pr comment 1 --body 'it'\''s; rm x'"
        );
        assert_eq!(command_text(HostProgram::Glab, &args(&[""])), "glab ''");
    }
}
