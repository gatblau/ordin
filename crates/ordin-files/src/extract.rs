//! Text extraction for the index and for the incoming-file hidden-text check (FileService.md
//! Internal Logic 5.4 and 11.3). Pure functions over bytes; the callers run them on a blocking
//! thread with a time limit.

use std::io::Read;

use crate::paths::{is_markdown_kind, is_office_zip_kind, is_text_kind};
use crate::{INDEX_MAX_SOURCE_BYTES, INDEX_TEXT_BYTES};

/// Why a file's text could not be read; the name is indexed alone and the class is logged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExtractFailure {
    Archive,
    Xml,
    Pdf,
}

impl ExtractFailure {
    pub(crate) fn class(self) -> &'static str {
        match self {
            ExtractFailure::Archive => "archive",
            ExtractFailure::Xml => "xml",
            ExtractFailure::Pdf => "pdf",
        }
    }
}

/// `Ok(None)`: the kind has no text (name only). `Ok(Some(text))`: at most `INDEX_TEXT_BYTES` of
/// text. A source above `INDEX_MAX_SOURCE_BYTES` is name only.
pub(crate) fn extract_text(kind: &str, bytes: &[u8]) -> Result<Option<String>, ExtractFailure> {
    if bytes.len() as u64 > INDEX_MAX_SOURCE_BYTES {
        return Ok(None);
    }
    let text = if is_markdown_kind(kind) {
        markdown_text(&String::from_utf8_lossy(bytes))
    } else if is_text_kind(kind) {
        String::from_utf8_lossy(bytes).into_owned()
    } else if is_office_zip_kind(kind) {
        office_text(kind, bytes, INDEX_TEXT_BYTES)?
    } else if kind == "pdf" {
        pdf_text(bytes)?
    } else {
        return Ok(None);
    };
    Ok(Some(cut(text, INDEX_TEXT_BYTES)))
}

/// The kinds `read_document` reads (FileService.md SA-FS-DOC-2): PDF, Word, PowerPoint, Excel,
/// Markdown and the plain-text kinds.
// spec: ordin-desktop/components/FileService#SA-FS-DOC-2
pub(crate) fn is_document_kind(kind: &str) -> bool {
    kind == "pdf" || is_office_zip_kind(kind) || is_markdown_kind(kind) || is_text_kind(kind)
}

/// The whole text of a document for `read_document`, up to `limit` bytes: Markdown keeps its
/// source (headings and tables mean something to an agent), the other kinds as for the index.
/// Ligatures a PDF stores as one character (`ﬁ`) become their letters. `Ok(None)`: not a
/// document kind.
// spec: ordin-desktop/components/FileService#SA-FS-DOC-2
pub(crate) fn document_text(
    kind: &str,
    bytes: &[u8],
    limit: usize,
) -> Result<Option<String>, ExtractFailure> {
    let text = if is_markdown_kind(kind) || is_text_kind(kind) {
        String::from_utf8_lossy(bytes).into_owned()
    } else if is_office_zip_kind(kind) {
        office_text(kind, bytes, limit)?
    } else if kind == "pdf" {
        unligature(&pdf_text(bytes)?)
    } else {
        return Ok(None);
    };
    Ok(Some(cut(text, limit)))
}

/// U+FB00–U+FB06, the Latin ligatures a PDF font may map one glyph to.
fn unligature(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\u{FB00}' => out.push_str("ff"),
            '\u{FB01}' => out.push_str("fi"),
            '\u{FB02}' => out.push_str("fl"),
            '\u{FB03}' => out.push_str("ffi"),
            '\u{FB04}' => out.push_str("ffl"),
            '\u{FB05}' | '\u{FB06}' => out.push_str("st"),
            other => out.push(other),
        }
    }
    out
}

/// Cuts at a character boundary at or below `limit` bytes.
pub(crate) fn cut(mut text: String, limit: usize) -> String {
    if text.len() > limit {
        let mut at = limit;
        while at > 0 && !text.is_char_boundary(at) {
            at -= 1;
        }
        text.truncate(at);
    }
    text
}

/// Markdown through `pulldown-cmark`, keeping text events only.
fn markdown_text(source: &str) -> String {
    use pulldown_cmark::{Event, Parser};
    let mut out = String::new();
    for event in Parser::new(source) {
        match event {
            Event::Text(t) | Event::Code(t) => {
                out.push_str(&t);
                out.push(' ');
            }
            Event::SoftBreak | Event::HardBreak => out.push(' '),
            _ => {}
        }
    }
    out
}

/// The text nodes of one XML part.
fn xml_text(xml: &[u8], out: &mut String) -> Result<(), ExtractFailure> {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_reader(xml);
    let mut buffer = Vec::new();
    loop {
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Text(t)) => {
                out.push_str(&t.xml10_content());
                out.push(' ');
            }
            // The end of a paragraph (`w:p` in Word, `a:p` in PowerPoint) ends its line.
            Ok(Event::End(e)) if e.local_name().into_inner() == "p" => {
                if out.ends_with(' ') {
                    out.pop();
                }
                out.push('\n');
            }
            Ok(Event::Eof) => return Ok(()),
            Ok(_) => {}
            Err(_) => return Err(ExtractFailure::Xml),
        }
        buffer.clear();
    }
}

/// `word/document.xml`, `ppt/slides/slide*.xml` or `xl/sharedStrings.xml` from the archive.
fn office_text(kind: &str, bytes: &[u8], limit: usize) -> Result<String, ExtractFailure> {
    let mut archive =
        zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|_| ExtractFailure::Archive)?;
    let mut wanted: Vec<String> = Vec::new();
    for name in archive.file_names().filter_map(Result::ok) {
        let take = match kind {
            "docx" => name == "word/document.xml",
            "xlsx" => name == "xl/sharedStrings.xml",
            _ => name.starts_with("ppt/slides/slide") && name.ends_with(".xml"),
        };
        if take {
            wanted.push(name.into_owned());
        }
    }
    wanted.sort();
    let mut out = String::new();
    for name in wanted {
        let mut part = Vec::new();
        archive
            .by_name(&name)
            .map_err(|_| ExtractFailure::Archive)?
            .take(INDEX_MAX_SOURCE_BYTES)
            .read_to_end(&mut part)
            .map_err(|_| ExtractFailure::Archive)?;
        xml_text(&part, &mut out)?;
        if out.len() > limit {
            break;
        }
    }
    Ok(out)
}

fn pdf_text(bytes: &[u8]) -> Result<String, ExtractFailure> {
    let mut document = lopdf::Document::load_mem(bytes).map_err(|_| ExtractFailure::Pdf)?;
    prefer_to_unicode(&mut document);
    let pages: Vec<u32> = document.get_pages().keys().copied().collect();
    document
        .extract_text(&pages)
        .map_err(|_| ExtractFailure::Pdf)
}

/// A simple font (Type 1, TrueType, Type 3) that carries a `/ToUnicode` map loses its
/// `/Encoding`, so the map is used. The library reads `/Encoding` first, and the fonts Chrome
/// writes for a system or variable font (a Mac page saved as PDF) name their glyphs `/g1`,
/// `/g2` in `/Differences`: those names mean nothing, and the text came out scrambled. The
/// `/ToUnicode` map is the PDF's own statement of which character each code is. Type 0 fonts
/// keep their `/Encoding` (`/Identity-H`), which the library already pairs with the map.
// spec: ordin-desktop/components/FileService#SA-FS-DOC-2
fn prefer_to_unicode(document: &mut lopdf::Document) {
    for object in document.objects.values_mut() {
        let lopdf::Object::Dictionary(font) = object else {
            continue;
        };
        let is_font = matches!(font.get(b"Type").and_then(lopdf::Object::as_name), Ok(b"Font"));
        let is_type0 = matches!(font.get(b"Subtype").and_then(lopdf::Object::as_name), Ok(b"Type0"));
        if is_font && !is_type0 && font.has(b"ToUnicode") {
            font.remove(b"Encoding");
        }
    }
}

/// True when the archive holds `vbaProject.bin` anywhere (Internal Logic 11.2).
pub(crate) fn holds_macros(bytes: &[u8]) -> bool {
    match zip::ZipArchive::new(std::io::Cursor::new(bytes)) {
        Ok(archive) => archive
            .file_names()
            .filter_map(Result::ok)
            .any(|name| name.rsplit('/').next() == Some("vbaProject.bin")),
        Err(_) => false,
    }
}

/// Internal Logic 11.3: a zero-width character or a Unicode tag character.
pub(crate) fn has_hidden_text(text: &str) -> bool {
    text.chars().any(|c| {
        matches!(c, '\u{200B}'..='\u{200D}' | '\u{2060}' | '\u{FEFF}' | '\u{E0000}'..='\u{E007F}')
    })
}

/// The first bytes are a program header (Internal Logic 11.2).
pub(crate) fn is_program_header(bytes: &[u8]) -> bool {
    const MAGIC: [&[u8]; 7] = [
        b"MZ",
        &[0x7F, b'E', b'L', b'F'],
        &[0xFE, 0xED, 0xFA, 0xCE],
        &[0xFE, 0xED, 0xFA, 0xCF],
        &[0xCE, 0xFA, 0xED, 0xFE],
        &[0xCF, 0xFA, 0xED, 0xFE],
        &[0xCA, 0xFE, 0xBA, 0xBE],
    ];
    MAGIC.iter().any(|m| bytes.starts_with(m))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn zip_with(name: &str, body: &str) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut writer = zip::ZipWriter::new(std::io::Cursor::new(&mut out));
            writer
                .start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            writer.write_all(body.as_bytes()).unwrap();
            writer.finish().unwrap();
        }
        out
    }

    /// One page whose font names its glyphs `/g1`..`/g3` and maps them with `/ToUnicode`, as
    /// Chrome writes a Type 3 font for a system font.
    fn pdf_with_named_glyphs() -> Vec<u8> {
        use lopdf::{dictionary, Document, Object, Stream};
        let mut doc = Document::with_version("1.5");
        let cmap = b"/CIDInit /ProcSet findresource begin
12 dict begin
begincmap
/CMapName /Adobe-Identity-UCS def
/CMapType 2 def
1 begincodespacerange
<00> <FF>
endcodespacerange
3 beginbfchar
<01> <005A>
<02> <00FC>
<03> <0072>
endbfchar
endcmap
CMapName currentdict /CMap defineresource pop
end
end
"
        .to_vec();
        let to_unicode = doc.add_object(Stream::new(dictionary! {}, cmap));
        let font = doc.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Helvetica",
            "Encoding" => dictionary! {
                "Type" => "Encoding",
                "Differences" => vec![1.into(), "g1".into(), "g2".into(), "g3".into()],
            },
            "ToUnicode" => to_unicode,
        });
        let content = doc.add_object(Stream::new(
            dictionary! {},
            b"BT /F1 12 Tf 72 700 Td (\x01\x02\x03) Tj ET".to_vec(),
        ));
        let pages = doc.new_object_id();
        let page = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages,
            "Contents" => content,
            "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } },
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        });
        doc.objects.insert(
            pages,
            Object::Dictionary(dictionary! {
                "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1,
            }),
        );
        let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
        doc.trailer.set("Root", catalog);
        let mut out = Vec::new();
        doc.save_to(&mut out).unwrap();
        out
    }

    // spec: ordin-desktop/components/FileService#SA-FS-DOC-2
    #[test]
    fn a_font_with_named_glyphs_is_read_through_its_unicode_map() {
        let text = document_text("pdf", &pdf_with_named_glyphs(), 10_000)
            .unwrap()
            .unwrap();
        assert!(text.contains("Zür"), "{text:?}");
    }

    #[test]
    fn kinds_without_text_are_name_only() {
        assert_eq!(extract_text("png", b"x").unwrap(), None);
        let big = vec![b'a'; (INDEX_MAX_SOURCE_BYTES + 1) as usize];
        assert_eq!(extract_text("txt", &big).unwrap(), None);
    }

    #[test]
    fn text_and_markdown() {
        assert_eq!(extract_text("txt", b"hello").unwrap().unwrap(), "hello");
        let md = extract_text("md", b"# Title\n\nSome *bold* `code`")
            .unwrap()
            .unwrap();
        assert!(md.contains("Title") && md.contains("bold") && md.contains("code"));
        assert!(!md.contains('#') && !md.contains('*'));
        let long = "é".repeat(INDEX_TEXT_BYTES);
        let cut = extract_text("txt", long.as_bytes()).unwrap().unwrap();
        assert!(cut.len() <= INDEX_TEXT_BYTES);
    }

    #[test]
    fn office_documents() {
        let docx = zip_with(
            "word/document.xml",
            "<w:p><w:t>Kestrel day rate</w:t></w:p>",
        );
        assert!(extract_text("docx", &docx)
            .unwrap()
            .unwrap()
            .contains("Kestrel day rate"));
        let xlsx = zip_with("xl/sharedStrings.xml", "<sst><si><t>Rates</t></si></sst>");
        assert!(extract_text("xlsx", &xlsx)
            .unwrap()
            .unwrap()
            .contains("Rates"));
        let pptx = zip_with("ppt/slides/slide1.xml", "<p><a:t>Plan</a:t></p>");
        assert!(extract_text("pptx", &pptx)
            .unwrap()
            .unwrap()
            .contains("Plan"));
        assert_eq!(
            extract_text("docx", b"not a zip").unwrap_err(),
            ExtractFailure::Archive
        );
        let broken = zip_with("word/document.xml", "<a><b></a>");
        assert_eq!(
            extract_text("docx", &broken).unwrap_err(),
            ExtractFailure::Xml
        );
        assert_eq!(ExtractFailure::Pdf.class(), "pdf");
    }

    #[test]
    fn pdf_failure_is_reported() {
        assert_eq!(
            extract_text("pdf", b"%PDF-broken").unwrap_err(),
            ExtractFailure::Pdf
        );
    }

    #[test]
    fn macros_hidden_text_and_headers() {
        assert!(holds_macros(&zip_with("word/vbaProject.bin", "x")));
        assert!(!holds_macros(&zip_with("word/document.xml", "x")));
        assert!(!holds_macros(b"plain"));
        assert!(has_hidden_text("a\u{200B}b"));
        assert!(has_hidden_text("a\u{E0041}"));
        assert!(!has_hidden_text("plain"));
        assert!(is_program_header(b"MZ\x90"));
        assert!(is_program_header(&[0xCA, 0xFE, 0xBA, 0xBE, 0]));
        assert!(!is_program_header(b"%PDF"));
    }
}
