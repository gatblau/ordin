//! Transcript files to utterances (TranscriptionService.md Internal Logic A2–A5): WebVTT cues, the
//! paragraphs of a Word document and plain text, then one normalisation for every source.

use std::io::Read as _;

use quick_xml::events::Event;
use quick_xml::Reader;

use super::error::TranscriptionError;
use super::types::Utterance;

/// A speaker name: 1–80 characters (A2, A4).
const MAX_SPEAKER_CHARS: usize = 80;
/// An utterance's text: at most 4,000 characters.
const MAX_TEXT_CHARS: usize = 4_000;
/// The last utterance with no end ends this long after its start (A4).
const LAST_UTTERANCE_MS: u64 = 5_000;
/// The most `word/document.xml` may unpack to *(chosen here)*: a 20 MiB Word file holds far less
/// text, so more is a compressed bomb, refused as too large.
const MAX_DOCX_XML_BYTES: u64 = 64 * 1024 * 1024;

/// An utterance before normalisation; `end` is `None` until the next start is known.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Raw {
    pub start: u64,
    pub end: Option<u64>,
    pub speaker: Option<String>,
    pub text: String,
}

// ---- times

/// `(\d{1,2}:)?\d{1,2}:\d{2}(\.\d{1,3})?` as milliseconds; WebVTT hours may have more digits.
pub(crate) fn parse_time(raw: &str) -> Option<u64> {
    let (clock, fraction) = match raw.split_once('.') {
        Some((clock, fraction)) => (clock, Some(fraction)),
        None => (raw, None),
    };
    let parts: Vec<&str> = clock.split(':').collect();
    let digits = |s: &str, min: usize, max: usize| {
        (min..=max).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_digit())
    };
    let (hours, minutes, seconds) = match parts.as_slice() {
        [m, s] if digits(m, 1, 2) && digits(s, 2, 2) => ("0", *m, *s),
        [h, m, s] if digits(h, 1, 3) && digits(m, 1, 2) && digits(s, 2, 2) => (*h, *m, *s),
        _ => return None,
    };
    let millis = match fraction {
        None => 0,
        Some(f) if digits(f, 1, 3) => {
            let padded = format!("{f:0<3}");
            padded.parse::<u64>().ok()?
        }
        Some(_) => return None,
    };
    let (h, m, s): (u64, u64, u64) = (
        hours.parse().ok()?,
        minutes.parse().ok()?,
        seconds.parse().ok()?,
    );
    if m > 59 || s > 59 {
        return None;
    }
    Some(((h * 60 + m) * 60 + s) * 1_000 + millis)
}

/// A time, or `time --> time` followed by anything (WebVTT cue settings), as `(start, end)`.
fn parse_time_line(line: &str) -> Option<(u64, Option<u64>)> {
    match line.split_once("-->") {
        Some((start, rest)) => {
            let start = parse_time(start.trim())?;
            let end = parse_time(rest.split_whitespace().next()?)?;
            Some((start, Some(end)))
        }
        None => parse_time(line.trim()).map(|start| (start, None)),
    }
}

// ---- speakers

/// 1–80 characters with no `:`, none of the sentence marks a spoken line ends with, and no word
/// starting with a lower-case letter *(the last two are chosen here, so a line of speech such as
/// "We meet again at 10:30" is kept as speech, never read as a name; a name that breaks them is
/// kept as speech too, so nothing said is lost)*.
fn is_name(raw: &str) -> bool {
    let chars = raw.chars().count();
    (1..=MAX_SPEAKER_CHARS).contains(&chars)
        && !raw.contains([':', '.', ',', '!', '?', ';'])
        && !raw.starts_with(|c: char| c.is_ascii_digit())
        && raw
            .split_whitespace()
            .all(|word| !word.starts_with(char::is_lowercase))
}

/// A leading `Name: ` (A2, A4), as the name and the rest of the line.
fn leading_speaker(line: &str) -> (Option<String>, &str) {
    if let Some((name, rest)) = line.split_once(": ") {
        let name = name.trim();
        let chars = name.chars().count();
        if (1..=MAX_SPEAKER_CHARS).contains(&chars) && !name.contains("://") {
            return (Some(name.to_owned()), rest);
        }
    }
    (None, line)
}

/// A name followed by a time on the same line, as `(name, start)`.
fn name_then_time(line: &str) -> Option<(String, u64)> {
    let (name, time) = line.trim().rsplit_once(char::is_whitespace)?;
    let start = parse_time(time)?;
    let name = name.trim();
    is_name(name).then(|| (name.to_owned(), start))
}

// ---- WebVTT (A2)

/// Removes `<…>` tags and decodes the three character references WebVTT text uses most.
fn plain_cue_text(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut in_tag = false;
    for c in raw.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
}

/// The name of a leading `<v Name>` voice tag (classes such as `<v.loud Name>` allowed).
fn voice_name(text: &str) -> Option<String> {
    let rest = text.trim_start().strip_prefix("<v")?;
    if !rest.starts_with([' ', '.', '\t']) {
        return None;
    }
    let tag = &rest[..rest.find('>')?];
    let name = tag
        .split_once(char::is_whitespace)
        .map_or("", |(_, n)| n)
        .trim();
    let chars = name.chars().count();
    (1..=MAX_SPEAKER_CHARS)
        .contains(&chars)
        .then(|| name.to_owned())
}

pub(crate) fn parse_vtt(text: &str) -> Result<Vec<Raw>, TranscriptionError> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut lines = text.lines().map(str::trim_end);
    let first = lines
        .by_ref()
        .find(|l| !l.trim().is_empty())
        .ok_or(TranscriptionError::TranscriptMalformed)?;
    if !first.starts_with("WEBVTT") {
        return Err(TranscriptionError::TranscriptMalformed);
    }
    let mut blocks: Vec<Vec<&str>> = Vec::new();
    let mut block: Vec<&str> = Vec::new();
    // The rest of the header block, up to the first blank line, is not a cue.
    let mut in_header = true;
    for line in lines {
        if line.trim().is_empty() {
            in_header = false;
            if !block.is_empty() {
                blocks.push(std::mem::take(&mut block));
            }
        } else if !in_header {
            block.push(line);
        }
    }
    if !block.is_empty() {
        blocks.push(block);
    }
    let mut out = Vec::new();
    for block in blocks {
        let head = block[0].trim_start();
        if ["NOTE", "STYLE", "REGION"].iter().any(|k| {
            head == *k || head.starts_with(&format!("{k} ")) || head.starts_with(&format!("{k}\t"))
        }) {
            continue;
        }
        let Some(timing_at) = block.iter().take(2).position(|l| l.contains("-->")) else {
            return Err(TranscriptionError::TranscriptMalformed);
        };
        let (start, end) =
            parse_time_line(block[timing_at]).ok_or(TranscriptionError::TranscriptMalformed)?;
        let joined = block[timing_at + 1..].join(" ");
        let (speaker, text) = match voice_name(&joined) {
            Some(name) => (Some(name), plain_cue_text(&joined)),
            None => {
                let plain = plain_cue_text(&joined);
                let (speaker, rest) = leading_speaker(&plain);
                (speaker, rest.to_owned())
            }
        };
        out.push(Raw {
            start,
            end,
            speaker,
            text,
        });
    }
    Ok(out)
}

// ---- Word (A3)

/// One line per `w:p`, each the `w:t` texts of the paragraph joined.
pub(crate) fn docx_lines(bytes: &[u8]) -> Result<Vec<String>, TranscriptionError> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|_| TranscriptionError::TranscriptMalformed)?;
    let mut xml = Vec::new();
    archive
        .by_name("word/document.xml")
        .map_err(|_| TranscriptionError::TranscriptMalformed)?
        .take(MAX_DOCX_XML_BYTES + 1)
        .read_to_end(&mut xml)
        .map_err(|_| TranscriptionError::TranscriptMalformed)?;
    if xml.len() as u64 > MAX_DOCX_XML_BYTES {
        return Err(TranscriptionError::FileTooLarge);
    }
    let xml = std::str::from_utf8(&xml).map_err(|_| TranscriptionError::TranscriptMalformed)?;
    let mut reader = Reader::from_str(xml);
    let mut lines = Vec::new();
    let mut line = String::new();
    let mut in_text = false;
    let mut depth: usize = 0;
    loop {
        match reader
            .read_event()
            .map_err(|_| TranscriptionError::TranscriptMalformed)?
        {
            Event::Start(start) => {
                depth += 1;
                match start.name().into_inner() {
                    "w:p" => line.clear(),
                    "w:t" => in_text = true,
                    _ => {}
                }
            }
            Event::End(end) => {
                depth = depth
                    .checked_sub(1)
                    .ok_or(TranscriptionError::TranscriptMalformed)?;
                match end.name().into_inner() {
                    "w:p" => lines.push(std::mem::take(&mut line)),
                    "w:t" => in_text = false,
                    _ => {}
                }
            }
            Event::Empty(empty) if empty.name().into_inner() == "w:p" => lines.push(String::new()),
            Event::Text(text) if in_text => line.push_str(&text.xml10_content()),
            Event::CData(text) if in_text => line.push_str(&text.xml10_content()),
            Event::GeneralRef(entity) if in_text => {
                match entity
                    .resolve_char_ref()
                    .map_err(|_| TranscriptionError::TranscriptMalformed)?
                {
                    Some(c) => line.push(c),
                    None => match entity.xml10_content().as_ref() {
                        "amp" => line.push('&'),
                        "lt" => line.push('<'),
                        "gt" => line.push('>'),
                        "quot" => line.push('"'),
                        "apos" => line.push('\''),
                        _ => return Err(TranscriptionError::TranscriptMalformed),
                    },
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if depth != 0 {
        return Err(TranscriptionError::TranscriptMalformed);
    }
    Ok(lines)
}

// ---- plain text (A4)

/// UTF-8 with an optional byte-order mark; anything else is malformed.
pub(crate) fn txt_lines(bytes: &[u8]) -> Result<Vec<String>, TranscriptionError> {
    let bytes = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    let text = std::str::from_utf8(bytes).map_err(|_| TranscriptionError::TranscriptMalformed)?;
    Ok(text.lines().map(str::to_owned).collect())
}

/// A4's grammar over the lines of a `.txt` or `.docx`.
pub(crate) fn parse_lines(lines: &[String]) -> Vec<Raw> {
    let lines: Vec<&str> = lines
        .iter()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect();
    let mut out = Vec::new();
    let mut pending: Option<(u64, Option<u64>)> = None;
    let mut speaker: Option<String> = None;
    let mut last_start = 0;
    // True after a name line: the next text line is that speaker's, taken whole, so speech holding
    // ": " is not read as a second name.
    let mut named = false;
    for (at, line) in lines.iter().enumerate() {
        if let Some(times) = parse_time_line(line) {
            pending = Some(times);
            continue;
        }
        if let Some((name, start)) = name_then_time(line) {
            speaker = Some(name);
            pending = Some((start, None));
            named = true;
            continue;
        }
        // A name alone, with the time on the next line. A line read while a time is still waiting
        // for its text is that text, never a name.
        let next_is_time = lines
            .get(at + 1)
            .is_some_and(|next| parse_time_line(next).is_some());
        if pending.is_none() && next_is_time && is_name(line) {
            speaker = Some((*line).to_owned());
            named = true;
            continue;
        }
        let (inline, text) = if std::mem::take(&mut named) {
            (None, *line)
        } else {
            leading_speaker(line)
        };
        if inline.is_some() {
            speaker = inline;
        }
        let (start, end) = pending.take().unwrap_or((last_start, None));
        last_start = start;
        out.push(Raw {
            start,
            end,
            speaker: speaker.clone(),
            text: text.to_owned(),
        });
    }
    out
}

// ---- normalisation (A5)

/// Runs of white space become one space; the ends are trimmed.
fn single_spaces(raw: &str) -> String {
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Text over 4,000 characters is kept as consecutive utterances with the same times, split between
/// words *(chosen here: nothing said is dropped)*.
fn split_long(text: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut part = String::new();
    let mut part_chars = 0;
    for word in text.split(' ') {
        for piece in word
            .chars()
            .collect::<Vec<_>>()
            .chunks(MAX_TEXT_CHARS)
            .map(|c| c.iter().collect::<String>())
        {
            let piece_chars = piece.chars().count();
            let joined = part_chars + usize::from(part_chars > 0) + piece_chars;
            if part_chars > 0 && joined > MAX_TEXT_CHARS {
                parts.push(std::mem::take(&mut part));
                part_chars = 0;
            }
            if part_chars > 0 {
                part.push(' ');
                part_chars += 1;
            }
            part.push_str(&piece);
            part_chars += piece_chars;
        }
    }
    if !part.is_empty() {
        parts.push(part);
    }
    parts
}

/// Ends filled in (A4), text and speakers normalised, empty utterances dropped, then ordered by
/// start time and speaker. No utterance left → `TRX_TRANSCRIPT_EMPTY`.
pub(crate) fn normalise(raw: Vec<Raw>) -> Result<Vec<Utterance>, TranscriptionError> {
    let starts: Vec<u64> = raw.iter().map(|r| r.start).collect();
    let mut out = Vec::with_capacity(raw.len());
    for (at, r) in raw.into_iter().enumerate() {
        let end = match r.end {
            Some(end) => end,
            None => starts
                .get(at + 1)
                .copied()
                .unwrap_or(r.start.saturating_add(LAST_UTTERANCE_MS)),
        }
        .max(r.start);
        let text = single_spaces(&r.text);
        if text.is_empty() {
            continue;
        }
        let speaker = r
            .speaker
            .map(|s| single_spaces(&s))
            .filter(|s| (1..=MAX_SPEAKER_CHARS).contains(&s.chars().count()));
        for part in split_long(&text) {
            out.push(Utterance {
                start_ms: r.start,
                end_ms: end,
                speaker: speaker.clone(),
                text: part,
            });
        }
    }
    if out.is_empty() {
        return Err(TranscriptionError::Empty);
    }
    out.sort_by(|a, b| {
        a.start_ms
            .cmp(&b.start_ms)
            .then_with(|| a.speaker.cmp(&b.speaker))
    });
    Ok(out)
}
