//! Unit tests of the grammar (TranscriptionService.md Internal Logic A2–A5) and the error contract.

use ordin_types::{CodedError, RepairAction};

use super::parse::{docx_lines, normalise, parse_lines, parse_time, parse_vtt, txt_lines, Raw};
use super::{utterances_of, TranscriptKind, TranscriptionError, ALL_CODES};

fn lines(text: &str) -> Vec<String> {
    text.lines().map(str::to_owned).collect()
}

fn raw(start: u64, end: Option<u64>, speaker: Option<&str>, text: &str) -> Raw {
    Raw {
        start,
        end,
        speaker: speaker.map(str::to_owned),
        text: text.to_owned(),
    }
}

/// A Word file holding `document` as `word/document.xml`.
fn docx(document: &str) -> Vec<u8> {
    use std::io::Write as _;
    let mut bytes = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut bytes);
        zip.start_file(
            "word/document.xml",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(document.as_bytes()).unwrap();
        zip.finish().unwrap();
    }
    bytes.into_inner()
}

#[test]
fn times_follow_the_grammar() {
    let cases: &[(&str, Option<u64>)] = &[
        ("0:01", Some(1_000)),
        ("12:34", Some(754_000)),
        ("1:02:03", Some(3_723_000)),
        ("00:00:04.2", Some(4_200)),
        ("00:00:04.25", Some(4_250)),
        ("00:00:04.250", Some(4_250)),
        ("100:00:00.000", Some(360_000_000)),
        ("0:1", None),
        ("0:60", None),
        ("60:00", None),
        ("0:00.1234", None),
        ("0:00.", None),
        ("a:00", None),
        ("", None),
        ("1:2:3:4", None),
    ];
    for (raw, want) in cases {
        assert_eq!(parse_time(raw), *want, "{raw}");
    }
}

#[test]
fn webvtt_needs_its_header() {
    assert!(matches!(
        parse_vtt("00:00:01.000 --> 00:00:02.000\nHi"),
        Err(TranscriptionError::TranscriptMalformed)
    ));
    assert!(matches!(
        parse_vtt("   \n\n"),
        Err(TranscriptionError::TranscriptMalformed)
    ));
}

#[test]
fn webvtt_cues_keep_their_speakers_and_skip_notes_styles_and_regions() {
    let text = "\u{feff}WEBVTT\nKind: captions\n\nSTYLE\n::cue { color: red }\n\nREGION\nid:r\n\n\
                NOTE a note\n\n00:01.000 --> 00:02.500 line:0\n<v.loud Sam Lee>Hello &amp; <b>welcome</b></v>\n\n\
                cue-2\n00:00:03.000 --> 00:00:04.000\nPriya Shah: Thanks\n\n\
                00:00:05.000 --> 00:00:06.000\n<v Sam Lee>Still me\n\n\
                00:00:07.000 --> 00:00:08.000\nNo speaker here";
    let cues = parse_vtt(text).unwrap();
    assert_eq!(
        cues,
        vec![
            raw(1_000, Some(2_500), Some("Sam Lee"), "Hello & welcome"),
            raw(3_000, Some(4_000), Some("Priya Shah"), "Thanks"),
            raw(5_000, Some(6_000), Some("Sam Lee"), "Still me"),
            raw(7_000, Some(8_000), None, "No speaker here"),
        ]
    );
}

#[test]
fn a_webvtt_cue_with_a_broken_time_is_malformed() {
    assert!(matches!(
        parse_vtt("WEBVTT\n\n00:00:xx --> 00:00:02.000\nHi"),
        Err(TranscriptionError::TranscriptMalformed)
    ));
    assert!(matches!(
        parse_vtt("WEBVTT\n\nid\nno timing\nHi"),
        Err(TranscriptionError::TranscriptMalformed)
    ));
}

#[test]
fn a_voice_name_over_80_characters_is_no_speaker() {
    let text = format!(
        "WEBVTT\n\n00:01.000 --> 00:02.000\n<v {}>Hello",
        "n".repeat(81)
    );
    assert_eq!(parse_vtt(&text).unwrap()[0].speaker, None);
}

#[test]
fn text_lines_take_times_and_speakers_in_each_layout() {
    // Teams-like: the name and time on one line, the speech on the next.
    let teams = parse_lines(&lines(
        "Sam Lee   0:01\nHello there: welcome.\nPriya Shah   0:05\nThanks.",
    ));
    assert_eq!(
        teams,
        vec![
            raw(1_000, None, Some("Sam Lee"), "Hello there: welcome."),
            raw(5_000, None, Some("Priya Shah"), "Thanks."),
        ]
    );
    // The name alone, then the time on the next line.
    let stacked = parse_lines(&lines(
        "Sam Lee\n0:01\nHello.\nPriya Shah\n0:05 --> 0:07\nThanks.",
    ));
    assert_eq!(
        stacked,
        vec![
            raw(1_000, None, Some("Sam Lee"), "Hello."),
            raw(5_000, Some(7_000), Some("Priya Shah"), "Thanks."),
        ]
    );
    // A range, then `Name: speech`.
    let ranged = parse_lines(&lines("00:00:01.000 --> 00:00:02.000\nSam Lee: Hello."));
    assert_eq!(
        ranged,
        vec![raw(1_000, Some(2_000), Some("Sam Lee"), "Hello.")]
    );
    // Speech with no time keeps the last start; speech ending in a sentence mark before a time is
    // still speech.
    let plain = parse_lines(&lines("0:01\nFirst line.\nSecond line.\n0:04\nThird line."));
    assert_eq!(
        plain,
        vec![
            raw(1_000, None, None, "First line."),
            raw(1_000, None, None, "Second line."),
            raw(4_000, None, None, "Third line."),
        ]
    );
    // A4 as written: a short line with no sentence mark right before a time is a speaker's name.
    let named = parse_lines(&lines("0:01\nFirst line.\nPriya Shah\n0:04\nThird line."));
    assert_eq!(
        named[1],
        raw(4_000, None, Some("Priya Shah"), "Third line.")
    );
    // Speech that ends in a time is not a name.
    let sentence = parse_lines(&lines("We meet again at 10:30\nfirst then\n0:05\nok"));
    assert_eq!(
        sentence,
        vec![
            raw(0, None, None, "We meet again at 10:30"),
            raw(0, None, None, "first then"),
            raw(5_000, None, None, "ok"),
        ]
    );
}

#[test]
fn plain_text_must_be_utf8_and_loses_its_byte_order_mark() {
    assert_eq!(
        txt_lines(b"\xef\xbb\xbfHi\nthere").unwrap(),
        ["Hi", "there"]
    );
    assert!(matches!(
        txt_lines(b"\xff\xfe"),
        Err(TranscriptionError::TranscriptMalformed)
    ));
}

#[test]
fn word_paragraphs_join_their_texts() {
    let document = "<w:document xmlns:w=\"w\"><w:body><w:p><w:r><w:t xml:space=\"preserve\">Fish </w:t>\
                    </w:r><w:r><w:t>&amp; chips &#233;</w:t></w:r></w:p><w:p/><w:p><w:r><w:t>Two</w:t>\
                    </w:r></w:p></w:body></w:document>";
    assert_eq!(
        docx_lines(&docx(document)).unwrap(),
        ["Fish & chips é", "", "Two"]
    );
}

#[test]
fn a_broken_word_file_is_malformed() {
    let cases: Vec<Vec<u8>> = vec![
        b"not a zip".to_vec(),
        docx("<w:document><w:p><w:t>open"),
        docx("<w:document><w:p><w:t>&bogus;</w:t></w:p></w:document>"),
    ];
    for bytes in cases {
        assert!(matches!(
            docx_lines(&bytes),
            Err(TranscriptionError::TranscriptMalformed)
        ));
    }
    // A Word file with no document part.
    use std::io::Write as _;
    let mut bytes = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut bytes);
        zip.start_file("other.xml", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"<a/>").unwrap();
        zip.finish().unwrap();
    }
    assert!(matches!(
        docx_lines(&bytes.into_inner()),
        Err(TranscriptionError::TranscriptMalformed)
    ));
}

#[test]
fn normalisation_fills_ends_spaces_drops_empty_and_orders() {
    let utterances = normalise(vec![
        raw(9_000, None, Some(" Sam  Lee "), "  last \t one "),
        raw(1_000, None, Some("Priya"), "first"),
        raw(5_000, Some(4_000), None, "   "),
        raw(5_000, Some(4_000), Some("Zed"), "end before start"),
        raw(5_000, None, Some("Ann"), "same start"),
    ])
    .unwrap();
    let got: Vec<(u64, u64, Option<&str>, &str)> = utterances
        .iter()
        .map(|u| (u.start_ms, u.end_ms, u.speaker.as_deref(), u.text.as_str()))
        .collect();
    assert_eq!(
        got,
        vec![
            (1_000, 5_000, Some("Priya"), "first"),
            // The last in the file ends 5 s after its start.
            (5_000, 10_000, Some("Ann"), "same start"),
            (5_000, 5_000, Some("Zed"), "end before start"),
            // Its end is the next start in the file (1 s), raised to its own start.
            (9_000, 9_000, Some("Sam Lee"), "last one"),
        ]
    );
    // The first utterance's end is the next start in document order, before sorting.
    let first = normalise(vec![
        raw(9_000, None, None, "a"),
        raw(1_000, None, None, "b"),
    ])
    .unwrap();
    assert_eq!(first[1].end_ms, 9_000);
}

#[test]
fn nothing_said_is_empty() {
    assert!(matches!(
        normalise(vec![raw(0, None, None, " \t ")]),
        Err(TranscriptionError::Empty)
    ));
    assert!(matches!(
        normalise(Vec::new()),
        Err(TranscriptionError::Empty)
    ));
}

#[test]
fn long_speech_is_split_between_words_and_nothing_is_dropped() {
    let text = format!("{} {}", "word ".repeat(1_000).trim_end(), "x".repeat(4_500));
    let utterances = normalise(vec![raw(0, Some(10), Some("Sam"), &text)]).unwrap();
    assert!(utterances.len() >= 3);
    for u in &utterances {
        let chars = u.text.chars().count();
        assert!((1..=4_000).contains(&chars), "{chars}");
        assert_eq!(
            (u.start_ms, u.end_ms, u.speaker.as_deref()),
            (0, 10, Some("Sam"))
        );
    }
    let joined: String = utterances
        .iter()
        .map(|u| u.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    assert_eq!(joined.replace(' ', ""), text.replace(' ', ""));
}

#[test]
fn each_kind_reaches_its_reader() {
    let vtt = utterances_of(
        TranscriptKind::Vtt,
        b"WEBVTT\n\n00:01.000 --> 00:02.000\nHi",
    )
    .unwrap();
    assert_eq!(vtt[0].text, "Hi");
    assert!(matches!(
        utterances_of(TranscriptKind::Vtt, b"WEBVTT\n\xff"),
        Err(TranscriptionError::TranscriptMalformed)
    ));
    let txt = utterances_of(TranscriptKind::Txt, b"0:01\nHi").unwrap();
    assert_eq!((txt[0].start_ms, txt[0].end_ms), (1_000, 6_000));
}

#[test]
fn every_error_has_its_code_retry_flag_and_repair() {
    let cases: Vec<(TranscriptionError, &str, bool)> = vec![
        (
            TranscriptionError::FormatUnsupported,
            "TRX_FORMAT_UNSUPPORTED",
            false,
        ),
        (
            TranscriptionError::FileUnreadable,
            "TRX_FILE_UNREADABLE",
            false,
        ),
        (
            TranscriptionError::FileTooLarge,
            "TRX_FILE_TOO_LARGE",
            false,
        ),
        (
            TranscriptionError::TranscriptMalformed,
            "TRX_TRANSCRIPT_MALFORMED",
            false,
        ),
        (TranscriptionError::Empty, "TRX_TRANSCRIPT_EMPTY", false),
        (
            TranscriptionError::AudioUndecodable,
            "TRX_AUDIO_UNDECODABLE",
            false,
        ),
        (TranscriptionError::ModelMissing, "TRX_MODEL_MISSING", false),
        (
            TranscriptionError::DownloadFailed,
            "TRX_MODEL_DOWNLOAD_FAILED",
            true,
        ),
        (
            TranscriptionError::ChecksumMismatch,
            "TRX_MODEL_CHECKSUM",
            true,
        ),
        (TranscriptionError::NoSpace, "TRX_NO_SPACE", false),
        (TranscriptionError::Cancelled, "TRX_CANCELLED", false),
        (TranscriptionError::Engine(-3), "TRX_ENGINE_FAILED", true),
        (TranscriptionError::Busy, "TRX_BUSY", true),
    ];
    assert_eq!(cases.len(), ALL_CODES.len());
    for (error, code, retryable) in cases {
        assert_eq!(error.code(), code);
        assert!(ALL_CODES.contains(&code));
        assert_eq!(error.retryable(), retryable, "{code}");
        assert_eq!(
            error.repair(),
            retryable.then_some(RepairAction::TryAgain),
            "{code}"
        );
    }
}
