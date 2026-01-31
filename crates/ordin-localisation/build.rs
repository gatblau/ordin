//! Build time of Localisation (Localisation.md Internal Logic A): reads every
//! `catalogues/<tag>/*.ftl`, refuses a bad catalogue (`LOC_BUILD_CATALOGUE_INVALID`), and emits the
//! message identifier constants, the safety set, the area table and each locale's concatenated text.
//!
//! The build fails with a message that names the file and line; it never skips a bad file.

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use fluent_syntax::ast::{
    Entry, Expression, InlineExpression, Message, Pattern, PatternElement, Resource,
};
use fluent_syntax::parser;
use fluent_syntax::serializer;

const SOURCE_LOCALE: &str = "en-GB";
const NATIVE_NAME_ID: &str = "locale-native-name";
const CODE: &str = "LOC_BUILD_CATALOGUE_INVALID";

// `main` is the build script's entry point; the crate's unit tests include this file and call `generate`.
#[allow(dead_code)]
fn main() {
    println!("cargo:rerun-if-changed=catalogues");
    println!("cargo:rerun-if-changed=build.rs");
    let result = (|| {
        let root = PathBuf::from(var("CARGO_MANIFEST_DIR")?).join("catalogues");
        let out = PathBuf::from(var("OUT_DIR")?);
        generate(
            &root,
            &out,
            env::var_os("CARGO_FEATURE_PSEUDO_LOCALES").is_some(),
        )
    })();
    if let Err(message) = result {
        // A failing build script is how the build fails (ADR-042); the message names file and line.
        eprintln!("error[{CODE}]: {message}");
        std::process::exit(1);
    }
}

/// One parsed locale folder.
struct Locale {
    tag: String,
    /// (file stem, source text) in file-name order.
    files: Vec<(String, String)>,
}

/// Reads `root/<tag>/*.ftl` and writes the generated files into `out`.
pub fn generate(root: &Path, out: &Path, pseudo_locales: bool) -> Result<(), String> {
    let mut locales = read_locales(root)?;
    let source = locales
        .iter()
        .find(|l| l.tag == SOURCE_LOCALE)
        .ok_or_else(|| format!("catalogues/{SOURCE_LOCALE} is missing"))?;

    // A3 / A3a: identifiers, safety set and areas of en-GB.
    let (ids, safety, areas) = collect_source(source)?;
    if safety.is_empty() {
        return Err(
            "the safety set is empty: no en-GB identifier begins `approval-` and no en-GB message carries `# safety`"
                .to_string(),
        );
    }

    // A4: every other locale: no orphan, and the native name. A4a (LOC-CMP-1): a locale is complete
    // when it holds every en-GB identifier; en-GB is complete by definition.
    let mut complete: BTreeSet<String> = BTreeSet::from([SOURCE_LOCALE.to_string()]);
    for locale in locales.iter().filter(|l| l.tag != SOURCE_LOCALE) {
        let seen = check_other(locale, &ids)?;
        if ids.iter().all(|id| seen.contains(id)) {
            complete.insert(locale.tag.clone());
        }
    }

    // A5: pseudo-locales.
    if pseudo_locales {
        let en_gb: Vec<(String, String)> = locales
            .iter()
            .find(|l| l.tag == SOURCE_LOCALE)
            .map(|l| l.files.clone())
            .unwrap_or_default();
        locales.push(pseudo("en-XA", &en_gb, pseudo_accent)?);
        locales.push(pseudo("ar-XB", &en_gb, pseudo_bidi)?);
        // Generated from en-GB, so complete by construction.
        complete.insert("en-XA".to_string());
        complete.insert("ar-XB".to_string());
    }

    // A6: the generated files.
    let mut ids_rs = String::new();
    for id in &ids {
        let constant = id.to_uppercase().replace('-', "_");
        let _ = writeln!(
            ids_rs,
            "pub const {constant}: crate::MessageId = crate::MessageId::new({id:?});"
        );
    }
    let _ = writeln!(ids_rs, "pub const MESSAGE_IDS: &[&str] = &{:?};", ids);
    let _ = writeln!(ids_rs, "pub const SAFETY_IDS: &[&str] = &{:?};", safety);
    let area_pairs: Vec<(&String, &String)> = areas.iter().collect();
    let _ = writeln!(
        ids_rs,
        "pub const MESSAGE_AREAS: &[(&str, &str)] = &{:?};",
        area_pairs
    );
    write(&out.join("message_ids.rs"), &ids_rs)?;

    // en-GB first, then alphabetical by tag.
    locales.sort_by(|a, b| (a.tag != SOURCE_LOCALE, &a.tag).cmp(&(b.tag != SOURCE_LOCALE, &b.tag)));
    let mut catalogues_rs =
        String::from("pub(crate) const CATALOGUE_TEXTS: &[(&str, &str)] = &[\n");
    for locale in &locales {
        let text = locale
            .files
            .iter()
            .map(|(_, text)| text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let file = format!("catalogue-{}.ftl", locale.tag);
        write(&out.join(&file), &text)?;
        let _ = writeln!(
            catalogues_rs,
            "    ({:?}, include_str!(concat!(env!(\"OUT_DIR\"), \"/{file}\"))),",
            locale.tag
        );
    }
    catalogues_rs.push_str("];\n");
    let complete: Vec<&str> = locales
        .iter()
        .map(|l| l.tag.as_str())
        .filter(|tag| complete.contains(*tag))
        .collect();
    let _ = writeln!(
        catalogues_rs,
        "pub(crate) const COMPLETE_LOCALES: &[&str] = &{complete:?};"
    );
    write(&out.join("catalogues.rs"), &catalogues_rs)?;
    Ok(())
}

fn var(name: &str) -> Result<String, String> {
    env::var(name).map_err(|_| format!("{name} is not set"))
}

fn write(path: &Path, text: &str) -> Result<(), String> {
    fs::write(path, text).map_err(|e| format!("cannot write {}: {e}", path.display()))
}

/// A1: every folder name must be a BCP 47 locale.
fn read_locales(root: &Path) -> Result<Vec<Locale>, String> {
    let mut folders: Vec<PathBuf> = fs::read_dir(root)
        .map_err(|e| format!("cannot read {}: {e}", root.display()))?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .collect();
    folders.sort();
    let mut locales = Vec::new();
    for folder in folders {
        let tag = folder
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| format!("{} has no usable name", folder.display()))?
            .to_string();
        if icu_locale_core::Locale::try_from_str(&tag).is_err() {
            return Err(format!("the folder name `{tag}` is not a BCP 47 locale"));
        }
        let mut paths: Vec<PathBuf> = fs::read_dir(&folder)
            .map_err(|e| format!("cannot read {}: {e}", folder.display()))?
            .filter_map(|entry| entry.ok().map(|e| e.path()))
            .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("ftl"))
            .collect();
        paths.sort();
        let mut files = Vec::new();
        for path in paths {
            let text = fs::read_to_string(&path)
                .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
            // A2: parse each file; any error fails the build with file, line and message.
            if let Err((_, errors)) = parser::parse(text.clone()) {
                let first = errors.first();
                let line = first.map_or(1, |e| line_of(&text, e.pos.start));
                let kind = first.map_or_else(String::new, |e| e.kind.to_string());
                return Err(format!("{}:{line}: {kind}", path.display()));
            }
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_string();
            files.push((stem, text));
        }
        locales.push(Locale { tag, files });
    }
    Ok(locales)
}

fn line_of(text: &str, offset: usize) -> usize {
    text.char_indices()
        .take_while(|(at, _)| *at < offset)
        .filter(|(_, c)| *c == '\n')
        .count()
        + 1
}

/// `^[a-z][a-z0-9]*(-[a-z0-9]+)*$` without a regex dependency.
fn is_kebab(id: &str) -> bool {
    let mut segments = id.split('-');
    let first_ok = segments.next().is_some_and(|s| {
        s.chars().next().is_some_and(|c| c.is_ascii_lowercase())
            && s.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
    });
    first_ok
        && segments.all(|s| {
            !s.is_empty()
                && s.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        })
}

type SourceFacts = (Vec<String>, Vec<String>, BTreeMap<String, String>);

/// A3 and A3a: identifiers (sorted), the safety set (sorted) and each identifier's area.
fn collect_source(locale: &Locale) -> Result<SourceFacts, String> {
    let mut areas: BTreeMap<String, String> = BTreeMap::new();
    let mut safety: BTreeSet<String> = BTreeSet::new();
    for (stem, text) in &locale.files {
        let resource = parser::parse(text.clone())
            .map_err(|_| format!("catalogues/{SOURCE_LOCALE}/{stem}.ftl does not parse"))?;
        for entry in &resource.body {
            if let Entry::Message(Message { id, comment, .. }) = entry {
                let id = id.name.clone();
                if !is_kebab(&id) {
                    return Err(format!(
                        "catalogues/{SOURCE_LOCALE}/{stem}.ftl: `{id}` is not kebab-case"
                    ));
                }
                if let Some(previous) = areas.insert(id.clone(), stem.clone()) {
                    return Err(format!(
                        "`{id}` is defined in both {previous}.ftl and {stem}.ftl of {SOURCE_LOCALE}"
                    ));
                }
                let marked = comment
                    .as_ref()
                    .is_some_and(|c| c.content.iter().any(|line| line == "safety"));
                if id.starts_with("approval-") || marked {
                    safety.insert(id);
                }
            }
        }
    }
    if !areas.contains_key(NATIVE_NAME_ID) {
        return Err(format!("{SOURCE_LOCALE} has no `{NATIVE_NAME_ID}`"));
    }
    let ids: Vec<String> = areas.keys().cloned().collect();
    Ok((ids, safety.into_iter().collect(), areas))
}

/// A3 (native name) and A4 (orphans); returns the identifiers the locale holds (A4a).
fn check_other(locale: &Locale, ids: &[String]) -> Result<BTreeSet<String>, String> {
    let known: BTreeSet<&str> = ids.iter().map(String::as_str).collect();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for (stem, text) in &locale.files {
        let resource = parser::parse(text.clone())
            .map_err(|_| format!("catalogues/{}/{stem}.ftl does not parse", locale.tag))?;
        for entry in &resource.body {
            if let Entry::Message(message) = entry {
                let id = message.id.name.as_str();
                if !known.contains(id) {
                    return Err(format!(
                        "catalogues/{}/{stem}.ftl: `{id}` is not in {SOURCE_LOCALE} (orphan message)",
                        locale.tag
                    ));
                }
                seen.insert(id.to_string());
            }
        }
    }
    if !seen.contains(NATIVE_NAME_ID) {
        return Err(format!("{} has no `{NATIVE_NAME_ID}`", locale.tag));
    }
    Ok(seen)
}

/// A5: a pseudo-locale made from en-GB by transforming every text element; placeables stay verbatim.
fn pseudo(
    tag: &str,
    files: &[(String, String)],
    text_map: fn(&str) -> String,
) -> Result<Locale, String> {
    let mut out = Vec::new();
    for (stem, text) in files {
        let mut resource: Resource<String> = parser::parse(text.clone())
            .map_err(|_| format!("catalogues/{SOURCE_LOCALE}/{stem}.ftl does not parse"))?;
        for entry in &mut resource.body {
            match entry {
                Entry::Message(message) => {
                    if let Some(pattern) = &mut message.value {
                        map_pattern(pattern, text_map);
                    }
                    for attribute in &mut message.attributes {
                        map_pattern(&mut attribute.value, text_map);
                    }
                }
                Entry::Term(term) => {
                    map_pattern(&mut term.value, text_map);
                    for attribute in &mut term.attributes {
                        map_pattern(&mut attribute.value, text_map);
                    }
                }
                _ => {}
            }
        }
        out.push((stem.clone(), serializer::serialize(&resource)));
    }
    Ok(Locale {
        tag: tag.to_string(),
        files: out,
    })
}

fn map_pattern(pattern: &mut Pattern<String>, text_map: fn(&str) -> String) {
    for element in &mut pattern.elements {
        match element {
            PatternElement::TextElement { value } => {
                if !value.trim().is_empty() {
                    *value = text_map(value);
                }
            }
            PatternElement::Placeable { expression } => map_expression(expression, text_map),
        }
    }
}

fn map_expression(expression: &mut Expression<String>, text_map: fn(&str) -> String) {
    match expression {
        Expression::Select { variants, .. } => {
            for variant in variants {
                map_pattern(&mut variant.value, text_map);
            }
        }
        Expression::Inline(InlineExpression::Placeable { expression }) => {
            map_expression(expression, text_map);
        }
        Expression::Inline(_) => {}
    }
}

/// Accented letters, then `·` padding to 140 % of the length in characters, rounded up.
fn pseudo_accent(value: &str) -> String {
    let mut text: String = value
        .chars()
        .map(|c| match c {
            'a' => 'á',
            'e' => 'é',
            'i' => 'í',
            'o' => 'ó',
            'u' => 'ú',
            'c' => 'ç',
            'n' => 'ñ',
            'A' => 'Á',
            'E' => 'É',
            'I' => 'Í',
            'O' => 'Ó',
            'U' => 'Ú',
            other => other,
        })
        .collect();
    let length = text.chars().count();
    let target = (length * 14).div_ceil(10);
    // Padding goes before trailing line breaks so a multi-line text keeps its shape.
    let trailing: String = text
        .chars()
        .rev()
        .take_while(|c| *c == '\n')
        .collect::<String>();
    text.truncate(text.len() - trailing.len());
    let have = text.chars().count();
    text.extend(std::iter::repeat_n(
        '·',
        target.saturating_sub(have + trailing.len()),
    ));
    text.push_str(&trailing);
    text
}

/// The text wrapped in right-to-left override (U+202E) and pop (U+202C).
fn pseudo_bidi(value: &str) -> String {
    let trimmed = value.trim_end_matches('\n');
    let tail = &value[trimmed.len()..];
    format!("\u{202E}{trimmed}\u{202C}{tail}")
}

/// For the crate's unit tests.
#[allow(dead_code)]
pub fn pseudo_accent_for_test(value: &str) -> String {
    pseudo_accent(value)
}
