//! What the three Office Open XML files share: escaping, the zip package and the document
//! properties.

use std::io::{Cursor, Write};

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

pub(super) const XML_HEAD: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#;
pub(super) const REL_NS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
pub(super) const OFFICE_REL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

/// `text` escaped for XML content and attribute values; characters XML 1.0 cannot hold are dropped.
pub(super) fn esc(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\t' | '\n' | '\r' => out.push(c),
            c if (c as u32) < 0x20 || c == '\u{FFFE}' || c == '\u{FFFF}' => {}
            c => out.push(c),
        }
    }
    out
}

/// A zip package written part by part, every part deflated.
pub(super) struct Package {
    zip: ZipWriter<Cursor<Vec<u8>>>,
}

impl Package {
    pub(super) fn new() -> Package {
        Package {
            zip: ZipWriter::new(Cursor::new(Vec::new())),
        }
    }

    /// Adds one part. Writing to memory does not fail; a failure would be a bug, and the part is
    /// then missing, which the tests see.
    pub(super) fn add(&mut self, name: &str, bytes: &[u8]) {
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        if self.zip.start_file(name, options).is_ok() {
            let _ = self.zip.write_all(bytes);
        }
    }

    pub(super) fn finish(self) -> Vec<u8> {
        self.zip
            .finish()
            .map(Cursor::into_inner)
            .unwrap_or_default()
    }
}

/// `_rels/.rels`: the main part, and the two property parts.
pub(super) fn root_rels(main_type: &str, main_target: &str) -> String {
    format!(
        r#"{XML_HEAD}<Relationships xmlns="{REL_NS}"><Relationship Id="rId1" Type="{OFFICE_REL}/{main_type}" Target="{main_target}"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/><Relationship Id="rId3" Type="{OFFICE_REL}/extended-properties" Target="docProps/app.xml"/></Relationships>"#
    )
}

/// `docProps/core.xml` with the title.
pub(super) fn core_props(title: &str) -> String {
    format!(
        r#"{XML_HEAD}<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"><dc:title>{}</dc:title><dc:creator>ordin</dc:creator></cp:coreProperties>"#,
        esc(title)
    )
}

/// `docProps/app.xml`.
pub(super) fn app_props() -> String {
    format!(
        r#"{XML_HEAD}<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties"><Application>ordin</Application></Properties>"#
    )
}

/// The `Default` and property `Override` entries every package's `[Content_Types].xml` holds,
/// followed by `overrides`.
pub(super) fn content_types(overrides: &[(String, &str)]) -> String {
    let mut out = format!(
        r#"{XML_HEAD}<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/><Override PartName="/docProps/app.xml" ContentType="application/vnd.openxmlformats-officedocument.extended-properties+xml"/>"#
    );
    for (part, content_type) in overrides {
        out.push_str(&format!(
            r#"<Override PartName="{part}" ContentType="{content_type}"/>"#
        ));
    }
    out.push_str("</Types>");
    out
}
