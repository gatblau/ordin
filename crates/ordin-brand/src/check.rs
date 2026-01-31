//! The pure checks of a brand pack (Brand.md *Internal Logic* C2–C5, and the 2C rules of the
//! identifier and version). Each failure is a [`Refusal`]: its kind and the en-GB reason sentence
//! stored in `refusal_reason`.
//!
//! This file depends on `std`, `sha2` and `unic-langid` only, because `build.rs` compiles it as well
//! and runs the same checks over the compiled-in build pack (Internal Logic A2).

use std::collections::BTreeMap;
use std::fmt::Write as _;

use sha2::{Digest as _, Sha256};

/// Why a pack was refused; each maps to one code and one reason sentence stored in `refusal_reason`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefusalKind {
    IdInvalid,
    VersionInvalid,
    StringsMissing,
    LocaleDuplicate,
    StringInvalid,
    TokensInvalid,
    FixedToken,
    ContrastFailed,
    UrlInvalid,
    AssetHashMismatch,
    AssetInvalid,
}

impl RefusalKind {
    /// The Error Table code of this kind.
    pub fn code(self) -> &'static str {
        match self {
            RefusalKind::IdInvalid => "BRAND_ID_INVALID",
            RefusalKind::VersionInvalid => "BRAND_VERSION_INVALID",
            RefusalKind::StringsMissing => "BRAND_STRINGS_MISSING",
            RefusalKind::LocaleDuplicate => "BRAND_LOCALE_DUPLICATE",
            RefusalKind::StringInvalid => "BRAND_STRING_INVALID",
            RefusalKind::TokensInvalid => "BRAND_TOKENS_INVALID",
            RefusalKind::FixedToken => "BRAND_FIXED_TOKEN",
            RefusalKind::ContrastFailed => "BRAND_CONTRAST_FAILED",
            RefusalKind::UrlInvalid => "BRAND_URL_INVALID",
            RefusalKind::AssetHashMismatch => "BRAND_ASSET_HASH_MISMATCH",
            RefusalKind::AssetInvalid => "BRAND_ASSET_INVALID",
        }
    }
}

impl std::fmt::Display for RefusalKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.code())
    }
}

/// One refusal: the kind and the reason sentence (≤ 500 characters, en-GB, never a URL).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    pub kind: RefusalKind,
    pub reason: String,
}

/// `refusal_reason` is at most 500 characters (2C rule table; `brand_packs` CHECK).
pub const MAX_REASON_CHARS: usize = 500;
/// A name or tag quoted in a reason is cut to this many characters.
const MAX_QUOTED_CHARS: usize = 64;

pub fn refusal(kind: RefusalKind, reason: String) -> Refusal {
    let reason = if reason.chars().count() > MAX_REASON_CHARS {
        let mut cut: String = reason.chars().take(MAX_REASON_CHARS - 1).collect();
        cut.push('…');
        cut
    } else {
        reason
    };
    Refusal { kind, reason }
}

/// A value from the pack placed in a reason sentence, cut so the sentence stays bounded.
fn quoted(value: &str) -> String {
    if value.chars().count() > MAX_QUOTED_CHARS {
        let mut cut: String = value.chars().take(MAX_QUOTED_CHARS - 1).collect();
        cut.push('…');
        cut
    } else {
        value.to_owned()
    }
}

// ---- identifier and version (2C rule table)

/// 1–64 characters of `[a-z0-9-]` (LD-07), else `BRAND_ID_INVALID`.
pub fn check_identifier(identifier: &str) -> Result<(), Refusal> {
    let valid = (1..=64).contains(&identifier.len())
        && identifier
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if valid {
        Ok(())
    } else {
        Err(refusal(
            RefusalKind::IdInvalid,
            "The brand identifier isn't valid.".to_owned(),
        ))
    }
}

/// `MAJOR.MINOR.PATCH`, each a decimal number without a leading zero, else `BRAND_VERSION_INVALID`.
pub fn check_version(version: &str) -> Result<(), Refusal> {
    let parts: Vec<&str> = version.split('.').collect();
    let number = |part: &&str| {
        !part.is_empty()
            && part.len() <= 9
            && part.bytes().all(|b| b.is_ascii_digit())
            && (part.len() == 1 || !part.starts_with('0'))
    };
    if parts.len() == 3 && parts.iter().all(number) {
        Ok(())
    } else {
        Err(refusal(
            RefusalKind::VersionInvalid,
            "The brand version isn't valid.".to_owned(),
        ))
    }
}

// ---- C2: assets

/// The asset slots of a pack, in fetch order (C1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetSlot {
    LogoLight,
    LogoDark,
    IconSetLight,
    IconSetDark,
    /// Zero-based position in the section's `fonts`.
    Font(usize),
}

/// At most 8 fonts (C2, assumed bound).
pub const MAX_FONTS: usize = 8;
const MIB: usize = 1024 * 1024;

impl AssetSlot {
    /// The asset's name in a reason sentence.
    // The Error Table's brackets mark the placeholder. The sentence reads "the light logo file", and
    // a font is numbered from 1 in the sentence ("font 2" is the second font, `index` 1).
    pub fn label(self) -> String {
        match self {
            AssetSlot::LogoLight => "light logo".to_owned(),
            AssetSlot::LogoDark => "dark logo".to_owned(),
            AssetSlot::IconSetLight => "light icon set".to_owned(),
            AssetSlot::IconSetDark => "dark icon set".to_owned(),
            AssetSlot::Font(index) => format!("font {}", index + 1),
        }
    }

    /// The `asset` field of a log line: the kind only, never the URL.
    pub fn kind(self) -> &'static str {
        match self {
            AssetSlot::LogoLight => "logo_light",
            AssetSlot::LogoDark => "logo_dark",
            AssetSlot::IconSetLight => "icon_set_light",
            AssetSlot::IconSetDark => "icon_set_dark",
            AssetSlot::Font(_) => "font",
        }
    }

    pub fn is_font(self) -> bool {
        matches!(self, AssetSlot::Font(_))
    }

    /// 2 MiB for logos and icon sets, 5 MiB per font (C2).
    pub fn max_bytes(self) -> usize {
        if self.is_font() {
            5 * MIB
        } else {
            2 * MIB
        }
    }

    /// The `AssetInvalid` refusal of this slot.
    pub fn invalid(self) -> Refusal {
        let what = if self.is_font() { "font" } else { "image" };
        refusal(
            RefusalKind::AssetInvalid,
            format!("The {} file isn't an accepted {what}.", self.label()),
        )
    }
}

/// More than [`MAX_FONTS`] fonts: the first one beyond the bound is refused as `AssetInvalid`.
pub fn check_font_count(count: usize) -> Result<(), Refusal> {
    if count > MAX_FONTS {
        Err(AssetSlot::Font(MAX_FONTS).invalid())
    } else {
        Ok(())
    }
}

/// Lowercase hexadecimal SHA-256 of `bytes`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for byte in digest {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// C2 for one asset body: size, SHA-256, media type against the magic bytes, and for an SVG no
/// active content or external reference. Returns the file extension of the accepted body.
pub fn check_asset(
    slot: AssetSlot,
    declared_sha256: &str,
    declared_media_type: &str,
    body: &[u8],
) -> Result<&'static str, Refusal> {
    if body.len() > slot.max_bytes() {
        return Err(slot.invalid());
    }
    if sha256_hex(body) != declared_sha256 {
        return Err(refusal(
            RefusalKind::AssetHashMismatch,
            format!(
                "The {} file does not match the brand's checksum.",
                slot.label()
            ),
        ));
    }
    let extension = match (slot.is_font(), declared_media_type) {
        (false, "image/svg+xml") if is_svg(body) => "svg",
        (false, "image/png") if body.starts_with(b"\x89PNG\r\n\x1a\n") => "png",
        (true, "font/woff2") if body.starts_with(b"wOF2") => "woff2",
        (true, "font/ttf")
            if body.starts_with(b"\x00\x01\x00\x00") || body.starts_with(b"true") =>
        {
            "ttf"
        }
        (true, "font/otf") if body.starts_with(b"OTTO") => "otf",
        _ => return Err(slot.invalid()),
    };
    if extension == "svg" && !svg_is_inert(body) {
        return Err(slot.invalid());
    }
    Ok(extension)
}

/// The magic of an SVG: UTF-8 text that opens with an XML declaration, a comment, a doctype or the
/// `<svg` element, and holds an `<svg` element.
fn is_svg(body: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(body) else {
        return false;
    };
    let text = text.trim_start_matches('\u{feff}').trim_start();
    let lower = text.to_ascii_lowercase();
    let opens = ["<?xml", "<svg", "<!--", "<!doctype svg"]
        .iter()
        .any(|start| lower.starts_with(start));
    opens && lower.contains("<svg")
}

/// C2: no `<script`, no `<foreignObject`, no attribute starting `on`, no `javascript:`, and no
/// `href`/`xlink:href` that does not start with `#` (case-insensitive).
pub fn svg_is_inert(body: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(body) else {
        return false;
    };
    let lower = text.to_ascii_lowercase();
    if ["<script", "<foreignobject", "javascript:"]
        .iter()
        .any(|needle| lower.contains(needle))
    {
        return false;
    }
    let bytes = lower.as_bytes();
    let mut at = 0;
    while let Some(offset) = lower[at..].find('<') {
        let start = at + offset + 1;
        let rest = &lower[start..];
        if rest.starts_with("!--") {
            at = match rest.find("-->") {
                Some(end) => start + end + 3,
                None => return true,
            };
            continue;
        }
        if rest.starts_with("![cdata[") {
            at = match rest.find("]]>") {
                Some(end) => start + end + 3,
                None => return true,
            };
            continue;
        }
        if !rest.starts_with(|c: char| c.is_ascii_alphabetic()) {
            at = start;
            continue;
        }
        match tag_attributes_inert(bytes, start) {
            Some((inert, end)) => {
                if !inert {
                    return false;
                }
                at = end;
            }
            None => return true,
        }
    }
    true
}

/// Reads one start tag from `start` (just after `<`); returns whether its attributes are inert and
/// the index after its `>`, or `None` at the end of the text.
fn tag_attributes_inert(bytes: &[u8], start: usize) -> Option<(bool, usize)> {
    let mut i = start;
    // The element name.
    while i < bytes.len() && !bytes[i].is_ascii_whitespace() && bytes[i] != b'>' && bytes[i] != b'/'
    {
        i += 1;
    }
    loop {
        while i < bytes.len() && (bytes[i].is_ascii_whitespace() || bytes[i] == b'/') {
            i += 1;
        }
        if i >= bytes.len() {
            return None;
        }
        if bytes[i] == b'>' {
            return Some((true, i + 1));
        }
        let name_start = i;
        while i < bytes.len()
            && !bytes[i].is_ascii_whitespace()
            && !matches!(bytes[i], b'=' | b'>' | b'/')
        {
            i += 1;
        }
        let name = String::from_utf8_lossy(&bytes[name_start..i]).into_owned();
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let mut value = String::new();
        if i < bytes.len() && bytes[i] == b'=' {
            i += 1;
            while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            if i < bytes.len() && (bytes[i] == b'"' || bytes[i] == b'\'') {
                let quote = bytes[i];
                let value_start = i + 1;
                i = value_start;
                while i < bytes.len() && bytes[i] != quote {
                    i += 1;
                }
                value =
                    String::from_utf8_lossy(&bytes[value_start..i.min(bytes.len())]).into_owned();
                i += 1;
            } else {
                let value_start = i;
                while i < bytes.len() && !bytes[i].is_ascii_whitespace() && bytes[i] != b'>' {
                    i += 1;
                }
                value = String::from_utf8_lossy(&bytes[value_start..i]).into_owned();
            }
        }
        if name.starts_with("on") {
            return Some((false, i));
        }
        if (name == "href" || name == "xlink:href") && !value.trim_start().starts_with('#') {
            return Some((false, i));
        }
    }
}

// ---- C3: strings

/// One locale's strings as the profile section gives them (`BrandStringsEntry`).
#[derive(Debug, Clone)]
pub struct EntryInput {
    pub product_name: String,
    pub coordinator_name: String,
    pub about_text: String,
}

/// One merged entry (`BrandStrings`), with its canonical locale tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergedStrings {
    pub locale: String,
    pub product_name: String,
    pub coordinator_name: String,
    pub about_text: String,
    pub legal_text: String,
}

/// The canonical form of a BCP 47 tag, or `None` if it is not one.
pub fn canonical_tag(tag: &str) -> Option<String> {
    tag.parse::<unic_langid::LanguageIdentifier>()
        .ok()
        .map(|id| id.to_string())
}

/// Canonicalises the keys of one map; a key that is no tag is `StringsMissing`, two keys equal after
/// canonicalisation are `LocaleDuplicate`.
fn canonical_keys<T: Clone>(map: &BTreeMap<String, T>) -> Result<BTreeMap<String, T>, Refusal> {
    let mut out = BTreeMap::new();
    for (tag, value) in map {
        // A tag that is not BCP 47 leaves that locale without usable strings, so it is refused as
        // BRAND_STRINGS_MISSING.
        let Some(canonical) = canonical_tag(tag) else {
            return Err(refusal(
                RefusalKind::StringsMissing,
                format!("The brand has no strings for {}.", quoted(tag)),
            ));
        };
        if out.insert(canonical.clone(), value.clone()).is_some() {
            return Err(refusal(
                RefusalKind::LocaleDuplicate,
                format!("The brand lists {} twice.", quoted(&canonical)),
            ));
        }
    }
    Ok(out)
}

fn check_length(text: &str, max: usize, field: &str, locale: &str) -> Result<(), Refusal> {
    let count = text.chars().count();
    if count == 0 {
        return Err(refusal(
            RefusalKind::StringInvalid,
            format!("The {field} for {} is empty.", quoted(locale)),
        ));
    }
    if count > max {
        return Err(refusal(
            RefusalKind::StringInvalid,
            format!("The {field} for {} is too long.", quoted(locale)),
        ));
    }
    Ok(())
}

/// C3: merges `strings` and `legal_text` into one entry per locale, sorted by locale.
pub fn merge_strings(
    strings: &BTreeMap<String, EntryInput>,
    legal_text: &BTreeMap<String, String>,
) -> Result<Vec<MergedStrings>, Refusal> {
    let strings = canonical_keys(strings)?;
    let legal_text = canonical_keys(legal_text)?;
    for locale in strings.keys() {
        if !legal_text.contains_key(locale) {
            return Err(refusal(
                RefusalKind::StringsMissing,
                format!("The brand has no legal text for {}.", quoted(locale)),
            ));
        }
    }
    for locale in legal_text.keys() {
        if !strings.contains_key(locale) {
            return Err(refusal(
                RefusalKind::StringsMissing,
                format!("The brand has no strings for {}.", quoted(locale)),
            ));
        }
    }
    if strings.is_empty() {
        return Err(refusal(
            RefusalKind::StringsMissing,
            "The brand has no strings.".to_owned(),
        ));
    }
    let mut merged = Vec::with_capacity(strings.len());
    for (locale, entry) in strings {
        let legal = legal_text.get(&locale).cloned().unwrap_or_default();
        check_length(&entry.product_name, 60, "product name", &locale)?;
        check_length(&entry.coordinator_name, 60, "coordinator name", &locale)?;
        check_length(&entry.about_text, 8000, "about text", &locale)?;
        check_length(&legal, 8000, "legal text", &locale)?;
        merged.push(MergedStrings {
            locale,
            product_name: entry.product_name,
            coordinator_name: entry.coordinator_name,
            about_text: entry.about_text,
            legal_text: legal,
        });
    }
    Ok(merged)
}

// ---- C4: tokens

/// At most 500 tokens, each value at most 256 characters (2C rule table).
pub const MAX_TOKENS: usize = 500;
pub const MAX_TOKEN_VALUE_CHARS: usize = 256;

/// Token names whose colours the application keeps fixed (ADR-041).
pub const FIXED_PREFIXES: [&str; 4] = [
    "color.danger.",
    "color.waiting.",
    "color.run-state.",
    "color.approval.",
];

/// The colour roles of the contrast check.
pub const COLOUR_ROLES: [&str; 8] = [
    "text",
    "text-muted",
    "background",
    "surface",
    "accent",
    "on-accent",
    "border-control",
    "focus",
];
pub const THEMES: [&str; 2] = ["light", "dark"];

/// The optional colour roles a pack may carry, each with the required role it falls back to when
/// the pack omits it (Brand.md, amendment January 2026, item 1; ADR-116 item 4).
pub const OPTIONAL_COLOUR_ROLES: [(&str, &str); 8] = [
    ("text-secondary", "text"),
    ("accent-ink", "text"),
    ("accent-soft", "surface"),
    ("chip", "surface"),
    ("line", "border-control"),
    ("line-strong", "border-control"),
    ("primary", "text"),
    ("on-primary", "background"),
];

/// The role whose value a pair reads: the role itself when the pack carries it, otherwise its
/// fallback (amendment January 2026, item 2: pairs are evaluated on the effective value).
fn effective_role(
    tokens: &BTreeMap<String, String>,
    role: &'static str,
    theme: &str,
) -> &'static str {
    match OPTIONAL_COLOUR_ROLES
        .iter()
        .find(|(optional, _)| *optional == role)
    {
        Some((_, fallback)) if !tokens.contains_key(&format!("color.{role}.{theme}")) => fallback,
        _ => role,
    }
}

fn tokens_invalid(reason: String) -> Refusal {
    refusal(RefusalKind::TokensInvalid, reason)
}

/// `^(color|font|space|radius)(\.[a-z0-9-]+){1,3}$`.
fn token_name_valid(name: &str) -> bool {
    let mut parts = name.split('.');
    let Some(namespace) = parts.next() else {
        return false;
    };
    if !["color", "font", "space", "radius"].contains(&namespace) {
        return false;
    }
    let rest: Vec<&str> = parts.collect();
    (1..=3).contains(&rest.len())
        && rest.iter().all(|segment| {
            !segment.is_empty()
                && segment
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
}

/// `^#[0-9A-Fa-f]{6}$`.
fn colour_value_valid(value: &str) -> bool {
    value.len() == 7 && value.starts_with('#') && value[1..].bytes().all(|b| b.is_ascii_hexdigit())
}

/// `^[0-9]{1,3}(\.[0-9])?px$`.
fn length_value_valid(value: &str) -> bool {
    let Some(number) = value.strip_suffix("px") else {
        return false;
    };
    let (whole, fraction) = match number.split_once('.') {
        Some((whole, fraction)) => (whole, Some(fraction)),
        None => (number, None),
    };
    let whole_ok = (1..=3).contains(&whole.len()) && whole.bytes().all(|b| b.is_ascii_digit());
    let fraction_ok = fraction.is_none_or(|f| f.len() == 1 && f.as_bytes()[0].is_ascii_digit());
    whole_ok && fraction_ok
}

/// A family name of 1–64 characters from `[A-Za-z0-9 -]`.
fn font_value_valid(value: &str) -> bool {
    (1..=64).contains(&value.len())
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b' ' || b == b'-')
}

/// C4: count, fixed prefixes, names, value forms and the required colour tokens.
pub fn check_tokens(tokens: &BTreeMap<String, String>) -> Result<(), Refusal> {
    if tokens.len() > MAX_TOKENS {
        return Err(tokens_invalid(format!(
            "The brand has more than {MAX_TOKENS} design tokens."
        )));
    }
    for name in tokens.keys() {
        if FIXED_PREFIXES.iter().any(|prefix| name.starts_with(prefix)) {
            return Err(refusal(
                RefusalKind::FixedToken,
                format!(
                    "The brand tries to change {}, which ordin keeps fixed.",
                    quoted(name)
                ),
            ));
        }
    }
    for (name, value) in tokens {
        if !token_name_valid(name) {
            return Err(tokens_invalid(format!(
                "The design token {} isn't allowed.",
                quoted(name)
            )));
        }
        let value_ok = value.chars().count() <= MAX_TOKEN_VALUE_CHARS
            && match name.split('.').next() {
                Some("color") => colour_value_valid(value),
                Some("space" | "radius") => length_value_valid(value),
                _ => font_value_valid(value),
            };
        if !value_ok {
            return Err(tokens_invalid(format!(
                "The design token {} has a value that isn't valid.",
                quoted(name)
            )));
        }
    }
    for theme in THEMES {
        for role in COLOUR_ROLES {
            let name = format!("color.{role}.{theme}");
            if !tokens.contains_key(&name) {
                return Err(tokens_invalid(format!(
                    "The design token {name} is missing."
                )));
            }
        }
    }
    Ok(())
}

// ---- C5: contrast

/// One pair of the contrast check: foreground role, background role, threshold and the sentence's
/// words for the pair.
struct Pair {
    foreground: &'static str,
    background: &'static str,
    threshold: f64,
    words: &'static str,
}

/// The fixed pairs of C5, in the table's order.
const PAIRS: [Pair; 8] = [
    Pair {
        foreground: "text",
        background: "background",
        threshold: 4.5,
        words: "Text colour on background",
    },
    Pair {
        foreground: "text",
        background: "surface",
        threshold: 4.5,
        words: "Text colour on surface",
    },
    Pair {
        foreground: "text-muted",
        background: "background",
        threshold: 4.5,
        words: "Muted text colour on background",
    },
    Pair {
        foreground: "text-muted",
        background: "surface",
        threshold: 4.5,
        words: "Muted text colour on surface",
    },
    Pair {
        foreground: "on-accent",
        background: "accent",
        threshold: 4.5,
        words: "Text colour on accent",
    },
    Pair {
        foreground: "accent",
        background: "background",
        threshold: 3.0,
        words: "Accent colour on background",
    },
    Pair {
        foreground: "border-control",
        background: "background",
        threshold: 3.0,
        words: "Control border colour on background",
    },
    Pair {
        foreground: "focus",
        background: "background",
        threshold: 3.0,
        words: "Focus indicator colour on background",
    },
];

/// The pairs the amendment of January 2026 adds (item 2), each evaluated on effective values.
const ADDED_PAIRS: [(&str, &str, f64); 8] = [
    ("text-secondary", "background", 4.5),
    ("text-secondary", "surface", 4.5),
    ("accent-ink", "background", 4.5),
    ("accent-ink", "surface", 4.5),
    ("accent-ink", "accent-soft", 4.5),
    ("text", "accent-soft", 4.5),
    ("text", "chip", 4.5),
    ("on-primary", "primary", 4.5),
];

/// The sentence's words for a pair of [`ADDED_PAIRS`], naming each role as read (a fallback is
/// named as its own role, as C5 names it).
fn added_pair_words(foreground: &str, background: &str) -> String {
    if (foreground, background) == ("on-primary", "primary") {
        return "Text colour on primary buttons".to_owned();
    }
    let front = match foreground {
        "text" => "Text colour",
        "text-secondary" => "Secondary text colour",
        "accent-ink" => "Accent text colour",
        "on-primary" => "Text colour on primary buttons",
        "background" => "Background colour",
        other => other,
    };
    let back = match background {
        "accent-soft" => "soft accent background",
        "chip" => "chip background",
        "primary" => "primary button colour",
        "text" => "text colour",
        other => other,
    };
    format!("{front} on {back}")
}

/// WCAG 2.2 relative luminance of `#RRGGBB`.
fn luminance(hex: &str) -> Option<f64> {
    if !colour_value_valid(hex) {
        return None;
    }
    let channel = |at: usize| -> Option<f64> {
        let value = u8::from_str_radix(&hex[at..at + 2], 16).ok()?;
        let c = f64::from(value) / 255.0;
        Some(if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        })
    };
    Some(0.2126 * channel(1)? + 0.7152 * channel(3)? + 0.0722 * channel(5)?)
}

/// WCAG 2.2 contrast ratio of two colours, lighter over darker.
pub fn contrast_ratio(first: &str, second: &str) -> Option<f64> {
    let a = luminance(first)?;
    let b = luminance(second)?;
    let (lighter, darker) = if a >= b { (a, b) } else { (b, a) };
    Some((lighter + 0.05) / (darker + 0.05))
}

/// A ratio quoted rounded down to one decimal.
fn one_decimal_down(ratio: f64) -> String {
    format!("{:.1}", (ratio * 10.0).floor() / 10.0)
}

/// A threshold quoted as WCAG writes it (4.5, 3).
fn threshold_text(threshold: f64) -> String {
    if threshold.fract() == 0.0 {
        format!("{threshold:.0}")
    } else {
        format!("{threshold:.1}")
    }
}

/// C5: every pair in both themes; the first failing pair's sentence, then "and N more".
/// Expects [`check_tokens`] to have passed (every required colour present and well formed).
pub fn check_contrast(tokens: &BTreeMap<String, String>) -> Result<(), Refusal> {
    let mut failing: Vec<String> = Vec::new();
    let mut measure =
        |words: &str, theme: &str, foreground: &str, background: &str, threshold: f64| {
            let colour = |role: &str| {
                tokens
                    .get(&format!("color.{role}.{theme}"))
                    .map_or(String::new(), String::clone)
            };
            match contrast_ratio(&colour(foreground), &colour(background)) {
                None => failing.push(format!("{words} in the {theme} theme can't be measured.")),
                Some(ratio) if ratio < threshold => failing.push(format!(
                    "{words} in the {theme} theme has contrast {}:1; at least {}:1 is needed.",
                    one_decimal_down(ratio),
                    threshold_text(threshold)
                )),
                Some(_) => {}
            }
        };
    for theme in THEMES {
        // A pair is measured once per theme: contrast is symmetric, and an added pair whose roles
        // fall back may name the same two roles as a pair already measured.
        let mut measured: Vec<(&str, &str)> = Vec::new();
        let mut first_time = |a: &'static str, b: &'static str| {
            let key = if a <= b { (a, b) } else { (b, a) };
            if measured.contains(&key) {
                false
            } else {
                measured.push(key);
                true
            }
        };
        for pair in &PAIRS {
            if first_time(pair.foreground, pair.background) {
                measure(
                    pair.words,
                    theme,
                    pair.foreground,
                    pair.background,
                    pair.threshold,
                );
            }
        }
        for (foreground, background, threshold) in ADDED_PAIRS {
            let foreground = effective_role(tokens, foreground, theme);
            let background = effective_role(tokens, background, theme);
            if first_time(foreground, background) {
                let words = added_pair_words(foreground, background);
                measure(&words, theme, foreground, background, threshold);
            }
        }
    }
    match failing.len() {
        0 => Ok(()),
        1 => Err(refusal(RefusalKind::ContrastFailed, failing.remove(0))),
        n => {
            let first = failing.remove(0);
            let first = first.strip_suffix('.').unwrap_or(&first);
            Err(refusal(
                RefusalKind::ContrastFailed,
                format!("{first}, and {} more.", n - 1),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn white_on_black_is_twenty_one_to_one() {
        let ratio = contrast_ratio("#FFFFFF", "#000000").unwrap();
        assert!((ratio - 21.0).abs() < 1e-9);
        assert_eq!(contrast_ratio("#000000", "#FFFFFF"), Some(ratio));
        assert_eq!(contrast_ratio("#FFF", "#000000"), None);
    }

    #[test]
    fn ratio_is_quoted_rounded_down() {
        assert_eq!(one_decimal_down(3.1182), "3.1");
        assert_eq!(one_decimal_down(4.4999), "4.4");
        assert_eq!(threshold_text(3.0), "3");
        assert_eq!(threshold_text(4.5), "4.5");
    }

    #[test]
    fn identifier_and_version_forms() {
        assert!(check_identifier("acme-assist").is_ok());
        assert!(check_identifier("").is_err());
        assert!(check_identifier("Acme").is_err());
        assert!(check_identifier(&"a".repeat(65)).is_err());
        assert!(check_version("2.1.0").is_ok());
        assert!(check_version("10.20.30").is_ok());
        for bad in ["2.1", "2.1.0.1", "v2.1.0", "02.1.0", "2..0", "2.1.x"] {
            assert!(check_version(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn token_name_and_value_forms() {
        assert!(token_name_valid("color.text.light"));
        assert!(token_name_valid("space.md"));
        assert!(!token_name_valid("color"));
        assert!(!token_name_valid("shadow.md"));
        assert!(!token_name_valid("color.a.b.c.d"));
        assert!(!token_name_valid("color.Text.light"));
        assert!(length_value_valid("8px"));
        assert!(length_value_valid("12.5px"));
        assert!(!length_value_valid("1234px"));
        assert!(!length_value_valid("1.25px"));
        assert!(!length_value_valid("8em"));
        assert!(font_value_valid("Inter Display"));
        assert!(!font_value_valid("Inter; drop"));
        assert!(colour_value_valid("#0b5fFF"));
        assert!(!colour_value_valid("0B5FFF"));
    }

    #[test]
    fn svg_scan_refuses_active_content_and_external_references() {
        let ok = br##"<?xml version="1.0"?><svg xmlns="http://www.w3.org/2000/svg"><use href="#a"/><!-- a comment --></svg>"##;
        assert!(svg_is_inert(ok));
        for bad in [
            &b"<svg><script>alert(1)</script></svg>"[..],
            b"<svg><foreignObject/></svg>",
            b"<svg onload=\"x()\"></svg>",
            b"<svg><a href=\"https://example.com\"/></svg>",
            b"<svg><image xlink:href='data:x'/></svg>",
            b"<svg><a href=\"javascript:x\"/></svg>",
            b"<svg><g OnClick=x></g></svg>",
        ] {
            assert!(!svg_is_inert(bad), "{}", String::from_utf8_lossy(bad));
        }
        assert!(is_svg(b"\xef\xbb\xbf <svg/>"));
        assert!(!is_svg(b"<html></html>"));
        assert!(!is_svg(b"\xff\xfe"));
    }

    #[test]
    fn asset_media_types_follow_the_magic_bytes() {
        let png = b"\x89PNG\r\n\x1a\nrest";
        let sha = sha256_hex(png);
        assert_eq!(
            check_asset(AssetSlot::LogoLight, &sha, "image/png", png),
            Ok("png")
        );
        assert!(check_asset(AssetSlot::LogoLight, &sha, "image/svg+xml", png).is_err());
        for (media, body, ext) in [
            ("font/woff2", &b"wOF2...."[..], "woff2"),
            ("font/ttf", b"\x00\x01\x00\x00....", "ttf"),
            ("font/otf", b"OTTO....", "otf"),
        ] {
            let sha = sha256_hex(body);
            assert_eq!(check_asset(AssetSlot::Font(0), &sha, media, body), Ok(ext));
            assert!(check_asset(AssetSlot::LogoDark, &sha, media, body).is_err());
        }
        assert_eq!(
            check_font_count(9).unwrap_err().reason,
            "The font 9 file isn't an accepted font."
        );
        assert!(check_font_count(8).is_ok());
    }

    #[test]
    fn a_long_reason_is_cut_to_five_hundred_characters() {
        let long = refusal(RefusalKind::TokensInvalid, "x".repeat(900));
        assert_eq!(long.reason.chars().count(), MAX_REASON_CHARS);
        assert_eq!(quoted(&"n".repeat(100)).chars().count(), MAX_QUOTED_CHARS);
    }
}
