//! The one reading of a shell command (RulesEngine.md *Internal Logic* D.5.4).
//!
//! It walks the command as the shell does, so that a quote, a comment or a substitution cannot put
//! it out of step, and reads what runs inside `$(…)`, backquotes, `<(…)`, subshells and the text
//! given to `eval` or `sh -c`. What it cannot know from the text alone it does not guess: the
//! reading stops being *plain*, and no rule lets a command through without a person on that
//! reading, so the worst case is a card.
//!
//! Plain is an allow-list, not a list of dangers: printable ASCII, words of ordinary characters,
//! single quotes, plain double quotes, the separators, and a program that is no shell built-in.
//! Everything else, known or not, is not plain.

/// How deep one command may nest substitutions before the reading gives up.
const MAX_DEPTH: usize = 16;

/// How many commands one reading holds. A command is at most 8,000 characters, but every word
/// after a wrapper is read as a program, so without this `sudo sudo sudo …` grows without end.
const MAX_SEGMENTS: usize = 1_024;

/// Words that open or close a compound command; the command proper follows them.
const KEYWORDS: &[&str] = &[
    "if",
    "then",
    "elif",
    "else",
    "fi",
    "do",
    "done",
    "while",
    "until",
    "{",
    "}",
    "!",
    "function",
    "select",
    "repeat",
    "noglob",
    "nocorrect",
    "-",
];

/// Shell built-ins. Several run text (`trap`, `let`, `read`, `compgen`, `declare`, `export`, `test`
/// with `-v`, `hash`, `alias`), so none but these reads plainly.
const SAFE_BUILTINS: &[&str] = &["echo", "true", "false", "pwd", "cd", ":"];

const BUILTINS: &[&str] = &[
    ".",
    "alias",
    "bg",
    "bind",
    "break",
    "builtin",
    "caller",
    "command",
    "compgen",
    "complete",
    "compopt",
    "continue",
    "coproc",
    "declare",
    "dirs",
    "disown",
    "enable",
    "eval",
    "exec",
    "exit",
    "export",
    "fc",
    "fg",
    "getopts",
    "hash",
    "help",
    "history",
    "jobs",
    "kill",
    "let",
    "local",
    "logout",
    "mapfile",
    "popd",
    "printf",
    "pushd",
    "read",
    "readarray",
    "readonly",
    "return",
    "set",
    "shift",
    "shopt",
    "source",
    "suspend",
    "test",
    "time",
    "times",
    "trap",
    "type",
    "typeset",
    "ulimit",
    "umask",
    "unalias",
    "unset",
    "wait",
    "autoload",
    "emulate",
    "float",
    "functions",
    "integer",
    "print",
    "rehash",
    "sched",
    "setopt",
    "unsetopt",
    "vared",
    "whence",
    "zcompile",
    "zformat",
    "zle",
    "zmodload",
    "zparseopts",
    "zstyle",
];

/// Programs that run the command named by their later words.
const WRAPPERS: &[&str] = &[
    "sudo",
    "doas",
    "env",
    "command",
    "builtin",
    "exec",
    "nohup",
    "nice",
    "ionice",
    "time",
    "timeout",
    "stdbuf",
    "setsid",
    "xargs",
    "parallel",
    "watch",
    "coproc",
    "caffeinate",
    "script",
];

/// Programs whose `-c` argument is shell text.
const SHELLS: &[&str] = &["sh", "bash", "dash", "zsh", "ksh", "fish", "csh", "tcsh"];

/// The flags of `find` that name a command.
const FIND_RUNS: &[&str] = &["-exec", "-execdir", "-ok", "-okdir"];

/// One command the shell would run.
#[derive(Debug, Clone, Default)]
pub(crate) struct Segment {
    /// The text as written, for the K.3 classification. Empty for a command found inside a word.
    pub(crate) raw: String,
    /// Every word, as the shell would pass it.
    pub(crate) words: Vec<String>,
    /// The words from the program on: compound keywords and leading assignments are gone.
    pub(crate) command: Vec<String>,
}

/// What a command runs, in reading order, nested commands included.
#[derive(Debug, Clone)]
pub(crate) struct Reading {
    pub(crate) segments: Vec<Segment>,
    /// The words above are all the shell will run, with nothing expanded at run time: no
    /// substitution, variable, glob, redirection to a file, here-document, assignment, compound
    /// command, wrapper or text given to another shell.
    pub(crate) plain: bool,
}

/// Reads a command.
pub(crate) fn read(command: &str) -> Reading {
    let mut lexer = Lexer {
        chars: command.chars().collect(),
        at: 0,
        depth: 0,
        heredocs: Vec::new(),
        segments: Vec::new(),
        plain: true,
    };
    lexer.list(None);
    let printable = command
        .chars()
        .all(|c| matches!(c, ' ' | '\t' | '\n') || c.is_ascii_graphic());
    Reading {
        plain: lexer.plain && printable,
        segments: lexer.segments,
    }
}

struct Lexer {
    chars: Vec<char>,
    at: usize,
    depth: usize,
    /// Here-document delimiters waiting for the next line break, with whether tabs are stripped and
    /// whether the delimiter was quoted.
    heredocs: Vec<(String, bool, bool)>,
    segments: Vec<Segment>,
    plain: bool,
}

impl Lexer {
    fn peek(&self, ahead: usize) -> Option<char> {
        self.chars.get(self.at + ahead).copied()
    }

    /// A command list, up to the end or the closing character.
    fn list(&mut self, until: Option<char>) {
        let mut start = self.at;
        let mut words: Vec<String> = Vec::new();
        loop {
            let Some(c) = self.peek(0) else {
                if until.is_some() {
                    self.plain = false;
                }
                self.finish(start, self.at, std::mem::take(&mut words));
                return;
            };
            if Some(c) == until {
                self.finish(start, self.at, std::mem::take(&mut words));
                self.at += 1;
                return;
            }
            match c {
                ' ' | '\t' => self.at += 1,
                '\n' => {
                    self.finish(start, self.at, std::mem::take(&mut words));
                    self.at += 1;
                    self.skip_heredocs();
                    start = self.at;
                }
                '#' => {
                    self.plain = false;
                    // Keeps the comment in `raw`, where K.3 refuses it: a comment is not a kind.
                    while self.peek(0).is_some_and(|next| next != '\n') {
                        self.at += 1;
                    }
                }
                ';' | '|' | ')' => {
                    self.plain &= c != ')';
                    self.finish(start, self.at, std::mem::take(&mut words));
                    self.at += 1;
                    // `||`, `|&`, `;;` and `;&` are one separator.
                    if matches!(self.peek(0), Some('|' | '&' | ';')) {
                        self.at += 1;
                    }
                    start = self.at;
                }
                '&' if self.peek(1) == Some('>') => self.redirection(),
                '&' => {
                    self.finish(start, self.at, std::mem::take(&mut words));
                    self.at += 1;
                    if self.peek(0) == Some('&') {
                        self.at += 1;
                    }
                    start = self.at;
                }
                '(' => {
                    self.plain = false;
                    self.finish(start, self.at, std::mem::take(&mut words));
                    self.at += 1;
                    self.nested();
                    start = self.at;
                }
                '<' | '>' if self.peek(1) == Some('(') => {
                    self.plain = false;
                    self.at += 2;
                    self.nested();
                }
                '<' | '>' => {
                    // A number written against the operator is the stream, not a word.
                    if words.last().is_some_and(|word| {
                        self.chars
                            .get(self.at.wrapping_sub(1))
                            .is_some_and(char::is_ascii_digit)
                            && word.chars().all(|d| d.is_ascii_digit())
                    }) {
                        words.pop();
                    }
                    self.redirection();
                }
                _ => {
                    if let Some(word) = self.word() {
                        words.push(word);
                    }
                }
            }
        }
    }

    /// A nested list closed by `)`, its commands added to the reading.
    fn nested(&mut self) {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            self.plain = false;
            self.at = self.chars.len();
        } else {
            self.list(Some(')'));
        }
        self.depth -= 1;
    }

    /// One redirection: the operator and its target, which is not a word of the command.
    fn redirection(&mut self) {
        let mut heredoc = None;
        let first = self.peek(0);
        self.at += 1;
        if first == Some('<') && self.peek(0) == Some('<') {
            self.at += 1;
            if self.peek(0) == Some('<') {
                self.at += 1;
            } else {
                let strip = self.peek(0) == Some('-');
                self.at += usize::from(strip);
                heredoc = Some(strip);
            }
        }
        while matches!(self.peek(0), Some('>' | '|')) {
            self.at += 1;
        }
        let duplicates = self.peek(0) == Some('&');
        self.at += usize::from(duplicates);
        while matches!(self.peek(0), Some(' ' | '\t')) {
            self.at += 1;
        }
        if let Some(strip) = heredoc {
            self.plain = false;
            let (delimiter, quoted) = self.heredoc_delimiter();
            self.heredocs.push((delimiter, strip, quoted));
            return;
        }
        let target = self.word().unwrap_or_default();
        let harmless = target == "/dev/null"
            || (duplicates
                && (target == "-"
                    || (!target.is_empty() && target.chars().all(|d| d.is_ascii_digit()))));
        self.plain &= harmless;
    }

    /// A here-document's delimiter as the shell takes it: the word with its quotes removed, nothing
    /// expanded. A quote anywhere in it leaves the body unexpanded.
    fn heredoc_delimiter(&mut self) -> (String, bool) {
        let mut delimiter = String::new();
        let mut quoted = false;
        while let Some(c) = self.peek(0) {
            if matches!(
                c,
                ' ' | '\t' | '\n' | ';' | '&' | '|' | '(' | ')' | '<' | '>'
            ) {
                break;
            }
            self.at += 1;
            match c {
                '\'' | '"' => quoted = true,
                '\\' => {
                    quoted = true;
                    delimiter.extend(self.peek(0));
                    self.at += 1;
                }
                _ => delimiter.push(c),
            }
        }
        (delimiter, quoted)
    }

    /// The bodies of the here-documents the last line opened. A body is data, or text for the
    /// program that reads it; an unquoted one still expands, so what it substitutes is read.
    fn skip_heredocs(&mut self) {
        for (delimiter, strip, quoted) in std::mem::take(&mut self.heredocs) {
            let start = self.at;
            let mut end_of_body = self.chars.len();
            while self.at < self.chars.len() {
                let line_start = self.at;
                let end = self.chars[self.at..]
                    .iter()
                    .position(|c| *c == '\n')
                    .map_or(self.chars.len(), |offset| self.at + offset);
                let line: String = self.chars[self.at..end].iter().collect();
                self.at = (end + 1).min(self.chars.len());
                let line = if strip {
                    line.trim_start_matches('\t')
                } else {
                    line.as_str()
                };
                if line == delimiter {
                    end_of_body = line_start;
                    break;
                }
            }
            if !quoted {
                let body: String = self.chars[start..end_of_body].iter().collect();
                self.expansions(&body);
            }
        }
    }

    /// What an unquoted here-document's body substitutes.
    fn expansions(&mut self, body: &str) {
        if self.depth >= MAX_DEPTH {
            return;
        }
        let mut lexer = Lexer {
            chars: body.chars().collect(),
            at: 0,
            depth: self.depth + 1,
            heredocs: Vec::new(),
            segments: std::mem::take(&mut self.segments),
            plain: false,
        };
        let mut ignored = String::new();
        while let Some(c) = lexer.peek(0) {
            lexer.at += 1;
            match c {
                '\\' => lexer.at += 1,
                '`' => lexer.backquoted(false),
                '$' => lexer.dollar(&mut ignored, true),
                _ => {}
            }
        }
        self.segments = lexer.segments;
    }

    /// One word with its quotes removed; None when it was only a quoteless nothing.
    fn word(&mut self) -> Option<String> {
        let mut text = String::new();
        let mut started = false;
        while let Some(c) = self.peek(0) {
            match c {
                ' ' | '\t' | '\n' | ';' | '&' | '|' | '(' | ')' | '<' | '>' => break,
                '\\' => {
                    self.plain = false;
                    self.at += 1;
                    match self.peek(0) {
                        Some('\n') => self.at += 1,
                        Some(next) => {
                            started = true;
                            text.push(next);
                            self.at += 1;
                        }
                        None => {
                            started = true;
                            text.push('\\');
                        }
                    }
                }
                '\'' => {
                    started = true;
                    self.at += 1;
                    self.single_quoted(&mut text);
                }
                '"' => {
                    started = true;
                    self.at += 1;
                    self.double_quoted(&mut text);
                }
                '`' => {
                    started = true;
                    self.at += 1;
                    self.backquoted(false);
                }
                '$' => {
                    started = true;
                    self.at += 1;
                    self.dollar(&mut text, false);
                }
                '*' | '?' | '[' | ']' | '~' | '!' | '^' => {
                    self.plain = false;
                    started = true;
                    text.push(c);
                    self.at += 1;
                }
                '{' | '}' if started => {
                    self.plain = false;
                    text.push(c);
                    self.at += 1;
                }
                '=' if !started => {
                    self.plain = false;
                    started = true;
                    text.push(c);
                    self.at += 1;
                }
                _ => {
                    started = true;
                    text.push(c);
                    self.at += 1;
                }
            }
        }
        started.then_some(text)
    }

    fn single_quoted(&mut self, text: &mut String) {
        while let Some(c) = self.peek(0) {
            self.at += 1;
            if c == '\'' {
                return;
            }
            text.push(c);
        }
        self.plain = false;
    }

    fn double_quoted(&mut self, text: &mut String) {
        while let Some(c) = self.peek(0) {
            self.at += 1;
            match c {
                '"' => return,
                '\\' => {
                    self.plain = false;
                    match self.peek(0) {
                        Some('"' | '\\' | '$' | '`') => {
                            text.extend(self.peek(0));
                            self.at += 1;
                        }
                        Some('\n') => self.at += 1,
                        _ => text.push(c),
                    }
                }
                '`' => self.backquoted(true),
                '$' => self.dollar(text, true),
                '!' | '~' => {
                    self.plain = false;
                    text.push(c);
                }
                _ => text.push(c),
            }
        }
        self.plain = false;
    }

    /// After a `$`. A lone `$` is a character; anything else expands at run time. Inside double
    /// quotes a following quote is a quote of its own, and `$` before it is a character.
    fn dollar(&mut self, text: &mut String, quoted: bool) {
        // Not plain whatever follows: a `$` that reads as a character here may not in another shell.
        self.plain = false;
        match self.peek(0) {
            Some('(') => {
                self.plain = false;
                self.at += 1;
                self.nested();
            }
            Some('{') => {
                self.plain = false;
                self.at += 1;
                self.parameter();
            }
            // ANSI quoting. Its escapes are not decoded: the word is not plain, so a card shows it.
            Some('\'') if !quoted => {
                self.plain = false;
                self.at += 1;
                while let Some(c) = self.peek(0) {
                    self.at += 1;
                    match c {
                        '\'' => return,
                        '\\' => {
                            text.extend(self.peek(0));
                            self.at += 1;
                        }
                        _ => text.push(c),
                    }
                }
            }
            // A translated string is a double-quoted one.
            Some('"') if !quoted => {
                self.at += 1;
                self.double_quoted(text);
            }
            Some(next) if next.is_ascii_alphanumeric() || "_@*#?$!-".contains(next) => {
                self.plain = false;
                self.at += 1;
                while self.peek(0).is_some_and(|c| {
                    next.is_ascii_alphabetic() | (next == '_')
                        && (c.is_ascii_alphanumeric() || c == '_')
                }) {
                    self.at += 1;
                }
            }
            _ => text.push('$'),
        }
    }

    /// `${…}`, whose default text may hold a substitution.
    fn parameter(&mut self) {
        let mut ignored = String::new();
        while let Some(c) = self.peek(0) {
            self.at += 1;
            match c {
                '}' => return,
                '\\' => self.at += 1,
                '\'' => self.single_quoted(&mut ignored),
                '"' => self.double_quoted(&mut ignored),
                '`' => self.backquoted(false),
                '$' => self.dollar(&mut ignored, false),
                _ => {}
            }
        }
    }

    /// After a backquote: the text up to the closing one is a command of its own. Inside double
    /// quotes a backslash also escapes a double quote there.
    fn backquoted(&mut self, quoted: bool) {
        self.plain = false;
        let mut inner = String::new();
        while let Some(c) = self.peek(0) {
            self.at += 1;
            match c {
                '`' => break,
                '\\' => match self.peek(0) {
                    Some(next @ ('`' | '\\' | '$')) => {
                        inner.push(next);
                        self.at += 1;
                    }
                    Some('"') if quoted => {
                        inner.push('"');
                        self.at += 1;
                    }
                    _ => inner.push(c),
                },
                _ => inner.push(c),
            }
        }
        self.text(&inner);
    }

    /// Reads shell text found inside a word or handed to another shell.
    fn text(&mut self, inner: &str) {
        self.plain = false;
        if self.depth >= MAX_DEPTH {
            return;
        }
        let mut lexer = Lexer {
            chars: inner.chars().collect(),
            at: 0,
            depth: self.depth + 1,
            heredocs: Vec::new(),
            segments: std::mem::take(&mut self.segments),
            plain: false,
        };
        lexer.list(None);
        self.segments = lexer.segments;
    }

    fn finish(&mut self, start: usize, end: usize, words: Vec<String>) {
        if words.is_empty() {
            return;
        }
        if self.segments.len() >= MAX_SEGMENTS {
            self.plain = false;
            return;
        }
        let raw: String = self.chars[start.min(end)..end].iter().collect();
        let command = self.command_of(&words);
        self.segments.push(Segment {
            raw,
            words,
            command: command.clone(),
        });
        self.follow(&command);
    }

    /// The words from the program on.
    fn command_of(&mut self, words: &[String]) -> Vec<String> {
        // `function f { y` names `f` before its command.
        if words.first().is_some_and(|word| word == "function") {
            self.plain = false;
            return self.command_of(words.get(2..).unwrap_or_default());
        }
        let skipped = words
            .iter()
            .take_while(|word| KEYWORDS.contains(&word.as_str()) || is_assignment(word))
            .count();
        self.plain &= skipped == 0;
        // `for x do y` has no separator before its command.
        if words.get(skipped).is_some_and(|word| word == "for") {
            self.plain = false;
            if let Some(at) = words.iter().position(|word| word == "do") {
                return words[at + 1..].to_vec();
            }
        }
        words[skipped..].to_vec()
    }

    /// Adds what a wrapper or another shell will run.
    fn follow(&mut self, command: &[String]) {
        let Some(program) = command.first().map(|word| program_name(word)) else {
            return;
        };
        if program.starts_with('-')
            || (BUILTINS.contains(&program.as_str()) && !SAFE_BUILTINS.contains(&program.as_str()))
        {
            self.plain = false;
        }
        if program == "eval" {
            let words: Vec<&str> = command[1..]
                .iter()
                .map(String::as_str)
                .filter(|word| *word != "--")
                .collect();
            self.text(&words.join(" "));
        } else if program == "case" {
            // Its patterns end in `)`, which only a full grammar tells from the end of a `$(`.
            self.plain = false;
        } else if SHELLS.contains(&program.as_str()) {
            self.plain = false;
            // Flags cluster (`-lc`, `-ce`), and the text is the first word after the flags.
            let has_c = command[1..].iter().any(|word| {
                word.len() > 1
                    && word.starts_with('-')
                    && !word.starts_with("--")
                    && word.contains('c')
            });
            if has_c {
                let text = command[1..].iter().find(|word| !word.starts_with('-'));
                if let Some(text) = text {
                    self.text(&text.clone());
                }
            }
        } else if WRAPPERS.contains(&program.as_str()) {
            self.after_wrapper(&command[1..]);
        } else if program == "find" {
            if let Some(at) = command.iter().position(|w| FIND_RUNS.contains(&w.as_str())) {
                self.after_wrapper(&command[at + 1..]);
            }
        }
    }

    /// Any later word may be the program, because each wrapper has its own flags with values:
    /// each is read as one, which over-reads only towards a refusal or a card.
    fn after_wrapper(&mut self, rest: &[String]) {
        self.plain = false;
        if self.depth >= MAX_DEPTH {
            return;
        }
        self.depth += 1;
        for at in 0..rest.len() {
            if self.segments.len() >= MAX_SEGMENTS {
                break;
            }
            // `env -S 'a b'` splits its value into a command.
            if rest[at].contains([' ', '\t']) {
                self.text(&rest[at].clone());
            }
            if rest[at].starts_with('-') || is_assignment(&rest[at]) {
                continue;
            }
            let command = rest[at..].to_vec();
            self.segments.push(Segment {
                raw: String::new(),
                words: command.clone(),
                command: command.clone(),
            });
            self.follow(&command);
        }
        self.depth -= 1;
    }
}

fn is_assignment(word: &str) -> bool {
    word.split_once('=').is_some_and(|(name, _)| {
        !name.is_empty()
            && !name.starts_with(|c: char| c.is_ascii_digit())
            && name
                .trim_end_matches('+')
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}

/// The program a word names: its last path part, compared as the file system would.
pub(crate) fn program_name(word: &str) -> String {
    let name = word.rsplit(['/', '\\']).next().unwrap_or(word);
    if cfg!(any(target_os = "macos", target_os = "windows")) {
        let lower = name.to_lowercase();
        lower.strip_suffix(".exe").unwrap_or(&lower).to_owned()
    } else {
        name.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn programs(command: &str) -> Vec<String> {
        read(command)
            .segments
            .iter()
            .filter_map(|segment| segment.command.first().map(|word| program_name(word)))
            .collect()
    }

    #[test]
    fn a_plain_command_stays_plain() {
        for command in [
            "cargo test",
            "cargo test -p x 2>&1 | tail -5",
            "git status && git diff > /dev/null",
            "echo 'a $(b) `c` ; d' \"e;f\"",
            "grep 'x*' src/lib.rs",
            "git commit -m 'a -- b'",
        ] {
            assert!(read(command).plain, "{command}");
        }
    }

    #[test]
    fn what_runs_at_run_time_is_not_plain() {
        for command in [
            "echo $(ls)",
            "echo `ls`",
            "echo \"$(ls)\"",
            "echo $HOME",
            "echo ${HOME}",
            "ls *.rs",
            "ls a?c",
            "ls {a,b}",
            "cat <(ls)",
            "(ls)",
            "X=1 ls",
            "ls > out",
            "ls >> out",
            "ls 2> err",
            "cat <<E\nbody\nE",
            "cat <<<x",
            "echo $'x'",
            "echo 'unclosed",
            "echo \"unclosed",
            "echo $(unclosed",
            "if true; then ls; fi",
            "sudo ls",
            "env ls",
            "xargs ls",
            "sh -c ls",
            "sh x.sh",
            "eval ls",
            "case a in b) ls;; esac",
            // Allow-listed out: reviewers ran each of these and something the reading did not list
            // ran, or the word was not what it showed.
            "echo a$",
            "echo x # y",
            "echo \"a$\"; ls; echo \"b\"",
            "trap 'ls' EXIT",
            "let 'a[$(ls)]=1'",
            "read 'a[$(ls)]'",
            "compgen -C 'ls' x",
            "declare x=1",
            "export x=1",
            "hash -p /usr/bin/touch ls",
            "alias x=ls",
            "shopt -s expand_aliases",
            "printf -v x y",
            "test -v 'a[$(ls)]'",
            "unset 'a[$(ls)]'",
            "true >&1-2",
            "echo ~",
            "echo \"$[1+1]\"",
            "\u{17f}h -c ls",
            "ba\u{17f}h -c ls",
            "csh -c ls",
            "tcsh -c ls",
            "caffeinate ls",
            "script -q /dev/null ls",
            "echo \\a",
            "echo \"\\a\"",
            "=touch x",
            "repeat 1 ls",
            "noglob ls",
            "- ls",
            "emulate sh -c 'ls'",
            "print -v 'a[$(ls)]' y",
            "-x ls",
        ] {
            assert!(!read(command).plain, "{command}");
        }
    }

    #[test]
    fn what_runs_inside_a_word_is_read() {
        for command in [
            "echo $(rm -rf x)",
            "echo `rm -rf x`",
            "echo \"$(rm -rf x)\"",
            "echo \"`rm -rf x`\"",
            "echo $(echo $(rm -rf x))",
            "echo `echo \\`rm -rf x\\``",
            "echo ${X:-$(rm -rf x)}",
            "echo \"${X:-`rm -rf x`}\"",
            "cat <(rm -rf x)",
            "echo > >(rm -rf x)",
            "(rm -rf x)",
            "{ rm -rf x; }",
            "if true; then rm -rf x; fi",
            "while true; do rm -rf x; done",
            "for a in b; do rm -rf x; done",
            "case a in b) rm -rf x;; esac",
            "f() { rm -rf x; }; f",
            "X=1 rm -rf x",
            "> out rm -rf x",
            "2>/dev/null rm -rf x",
            "sudo rm -rf x",
            "env A=1 rm -rf x",
            "timeout 5 rm -rf x",
            "xargs rm -rf",
            "nohup nice rm -rf x",
            "find . -exec rm -rf {} ;",
            "sh -c 'rm -rf x'",
            "bash -lc 'rm -rf x'",
            "eval rm -rf x",
            "eval 'rm -rf x'",
            "/bin/rm -rf x",
            "r\"\"m -rf x",
            "\\rm -rf x",
            "'rm' -rf x",
            "echo \"a$\"; rm -rf x; echo \"b\"",
            "echo \"$'\"; rm -rf x; echo \"'\"",
            "echo ${x:-{}; rm -rf x; echo ${y:-}}",
            "echo \"`echo \\\"'\\\"; rm -rf x; echo \\\"'\\\"`\"",
            "bash -c -- 'rm -rf x'",
            "bash -ce 'rm -rf x'",
            "eval -- 'rm -rf x'",
            "env -S 'rm -rf x'",
            "function f { rm -rf x; }; f",
            "set -- a; for x do rm -rf x; done",
            "cat <<E\n$(rm -rf x)\nE",
            "cat <<$E\n$E\nrm -rf x\n",
            "- rm -rf x",
        ] {
            assert!(programs(command).contains(&"rm".to_owned()), "{command}");
        }
    }

    #[test]
    fn a_comment_or_ansi_quote_keeps_the_reading_in_step() {
        // The quote is inside a comment, so the line after it runs.
        assert!(programs("true # '\nrm -rf x # '").contains(&"rm".to_owned()));
        // `$'\''` is one quote, so the `;` after it separates.
        assert!(programs("echo $'\\''; rm -rf x").contains(&"rm".to_owned()));
        assert!(programs("echo $\"a\"; rm -rf x").contains(&"rm".to_owned()));
        // A `#` inside a word starts no comment.
        assert!(programs("echo a#b; rm -rf x").contains(&"rm".to_owned()));
    }

    #[test]
    fn a_here_document_body_is_not_a_command_but_what_follows_is() {
        let command = "cat <<E\n'\nrm body\nE\nrm -rf x";
        let reading = read(command);
        assert!(!reading.plain);
        assert_eq!(programs(command), vec!["cat", "rm"]);
        assert_eq!(reading.segments[1].words, vec!["rm", "-rf", "x"]);
        assert_eq!(programs("cat <<-E\n\trm\n\tE\nls"), vec!["cat", "ls"]);
        assert_eq!(programs("cat <<'E'\nrm\nE"), vec!["cat"]);
        assert_eq!(programs("cat <<'E'\n$(rm)\nE"), vec!["cat"]);
    }

    #[test]
    fn a_stream_number_is_not_a_word_but_another_number_is() {
        assert_eq!(read("ls 2>&1").segments[0].words, vec!["ls"]);
        assert_eq!(
            read("sleep 2 > /dev/null").segments[0].words,
            vec!["sleep", "2"]
        );
    }

    #[test]
    fn a_nesting_bomb_ends() {
        let command = format!("{}x{}", "$(".repeat(5_000), ")".repeat(5_000));
        assert!(!read(&command).plain);
        let command = format!("{}x", "sudo ".repeat(4));
        assert!(!read(&command).plain);
        let command = "sudo ".repeat(1_600);
        let reading = read(&command);
        assert!(!reading.plain);
        assert!(reading.segments.len() <= MAX_SEGMENTS);
    }
}
