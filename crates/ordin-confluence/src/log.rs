//! Failure events that would otherwise be lost (Logging.md). Each event is one `tracing` call whose
//! message is the event name. No event carries a page title, page content, a path, an address, a site
//! name or a raw error message: only ids, codes, operation names, classifications and timings.

/// A page came back with no storage body, so an empty file is written in its place. `form` says what
/// was found: `no_body`, `no_storage` or `empty_storage`.
pub(crate) fn page_body_missing(page_id: &str, form: &'static str) {
    tracing::warn!(
        component = "confluence",
        page_id,
        form,
        "confluence.page_body_missing"
    );
}

/// A request had no reply. `failure` is a classification: `timeout`, `connect`, `reply_body`, `other`.
pub(crate) fn request_failed(
    operation: &str,
    failure: &'static str,
    elapsed_ms: u64,
    attempt: u64,
) {
    tracing::warn!(
        component = "confluence",
        operation,
        failure,
        elapsed_ms,
        attempt,
        "confluence.request_failed"
    );
}

/// A best-effort write to the adapter's own tables failed. `step` names the write.
pub(crate) fn store_write_failed(step: &'static str, code: &str) {
    tracing::warn!(
        component = "confluence",
        step,
        code,
        "confluence.store_write_failed"
    );
}

/// Marking a refused API token as needing a new one failed.
pub(crate) fn token_report_failed(code: &str) {
    tracing::warn!(
        component = "confluence",
        code,
        "confluence.token_report_failed"
    );
}

/// An author's name could not be read; the item names nobody. `reason` is `call_failed` or
/// `reply_invalid`.
pub(crate) fn author_lookup_failed(reason: &'static str, code: &str) {
    tracing::warn!(
        component = "confluence",
        reason,
        code,
        "confluence.author_lookup_failed"
    );
}

/// A Word file could not be converted to Markdown, so it is attached instead.
pub(crate) fn word_conversion_failed(bytes: u64) {
    tracing::warn!(
        component = "confluence",
        bytes,
        "confluence.word_conversion_failed"
    );
}

/// The Journal refused the record of an external action.
pub(crate) fn journal_begin_failed(code: &str) {
    tracing::warn!(
        component = "confluence",
        code,
        "confluence.journal_begin_failed"
    );
}

/// The classification of a request that got no reply. `reqwest`'s own message holds the address, so
/// only its kind is kept.
pub(crate) fn failure_of(error: &reqwest::Error) -> &'static str {
    if error.is_timeout() {
        "timeout"
    } else if error.is_connect() {
        "connect"
    } else if error.is_body() || error.is_decode() {
        "reply_body"
    } else {
        "other"
    }
}

#[cfg(test)]
mod tests {
    #[derive(Clone, Default)]
    struct Lines(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);
    impl std::io::Write for Lines {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().expect("lines").extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn captured(emit: impl FnOnce()) -> String {
        let lines = Lines::default();
        let writer = lines.clone();
        let subscriber = ordin_types::log::subscriber(move || writer.clone(), tracing::Level::INFO);
        tracing::subscriber::with_default(subscriber, emit);
        let text = String::from_utf8(lines.0.lock().expect("lines").clone()).expect("utf-8");
        text
    }

    #[test]
    fn every_failure_event_is_named_and_readable() {
        type Case = (&'static str, fn(), &'static str, &'static [&'static str]);
        let cases: [Case; 7] = [
            (
                "confluence.page_body_missing",
                || super::page_body_missing("12345", "no_storage"),
                "page_body_missing",
                &[r#""page_id":"12345""#, r#""form":"no_storage""#],
            ),
            (
                "confluence.request_failed",
                || super::request_failed("page_read", "timeout", 1500, 2),
                "request_failed",
                &[
                    r#""operation":"page_read""#,
                    r#""failure":"timeout""#,
                    r#""elapsed_ms":1500"#,
                    r#""attempt":2"#,
                ],
            ),
            (
                "confluence.store_write_failed",
                || super::store_write_failed("item_version", "CONF_DATASTORE_UNAVAILABLE"),
                "store_write_failed",
                &[r#""step":"item_version""#, "CONF_DATASTORE_UNAVAILABLE"],
            ),
            (
                "confluence.token_report_failed",
                || super::token_report_failed("CRED_NOT_FOUND"),
                "token_report_failed",
                &[r#""code":"CRED_NOT_FOUND""#],
            ),
            (
                "confluence.author_lookup_failed",
                || super::author_lookup_failed("reply_invalid", "CONF_RESPONSE_INVALID"),
                "author_lookup_failed",
                &[r#""reason":"reply_invalid""#],
            ),
            (
                "confluence.word_conversion_failed",
                || super::word_conversion_failed(2048),
                "word_conversion_failed",
                &[r#""bytes":2048"#],
            ),
            (
                "confluence.journal_begin_failed",
                || super::journal_begin_failed("JOURNAL_UNAVAILABLE"),
                "journal_begin_failed",
                &[r#""code":"JOURNAL_UNAVAILABLE""#],
            ),
        ];
        for (event, emit, label, fields) in cases {
            let text = captured(emit);
            assert!(
                text.contains(&format!(r#""event":"{event}""#)),
                "{label}: {text}"
            );
            assert!(!text.contains("invalid_event_name"), "{label}: {text}");
            assert!(!text.contains("<debug>"), "{label}: {text}");
            for field in fields {
                assert!(text.contains(field), "{label} lacks {field}: {text}");
            }
        }
    }
}
