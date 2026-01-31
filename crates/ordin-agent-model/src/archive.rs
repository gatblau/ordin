//! The canonical archive and the content digest (AgentModel.md *Internal Logic* 5).
//!
//! The archive is an uncompressed POSIX ustar archive with every variable field fixed, so the same
//! part files always give the same bytes, and so the same SHA-256.

use sha2::{Digest, Sha256};

use ordin_types::num::Sha256Hex;

use crate::error::AmError;
use crate::tree::PartFiles;

const BLOCK: usize = 512;
const NAME_LEN: usize = 100;
const PREFIX_LEN: usize = 155;
/// The longest name ustar can hold through `prefix` and `name` (255 bytes, as the spec bounds it).
const MAX_PATH_LEN: usize = 255;
const DIGEST_PREFIX: &str = "sha256:";

/// Splits `name` into ustar's `prefix` and `name` fields; None when it cannot be represented (a
/// component over 100 bytes, or a total over 255 bytes).
pub(crate) fn ustar_split(name: &str) -> Option<(&str, &str)> {
    if name.is_empty() || name.len() > MAX_PATH_LEN || name.split('/').any(|c| c.len() > NAME_LEN) {
        return None;
    }
    if name.len() <= NAME_LEN {
        return Some(("", name));
    }
    // The longest prefix that fits, as the common ustar writers choose it.
    let limit = name.len().min(PREFIX_LEN + 1);
    let at = name.as_bytes()[..limit].iter().rposition(|b| *b == b'/')?;
    let (prefix, rest) = (&name[..at], &name[at + 1..]);
    (at > 0 && !rest.is_empty() && rest.len() <= NAME_LEN && prefix.len() <= PREFIX_LEN)
        .then_some((prefix, rest))
}

/// Writes `value` as `width - 1` octal digits and a NUL.
fn octal(field: &mut [u8], value: u64) {
    let width = field.len() - 1;
    let text = format!("{value:0width$o}");
    let bytes = text.as_bytes();
    let start = bytes.len().saturating_sub(width);
    field[..width].copy_from_slice(&bytes[start..]);
    field[width] = 0;
}

fn header(name: &str, size: u64) -> [u8; BLOCK] {
    let mut block = [0u8; BLOCK];
    // A name that cannot be split is refused by the caller before this is reached (step 5); were one
    // to arrive, its last 100 bytes stand in the `name` field so the function stays total.
    let (prefix, short): (&[u8], &[u8]) = match ustar_split(name) {
        Some((prefix, short)) => (prefix.as_bytes(), short.as_bytes()),
        None => (b"", &name.as_bytes()[name.len().saturating_sub(NAME_LEN)..]),
    };
    block[..short.len()].copy_from_slice(short);
    octal(&mut block[100..108], 0o644); // mode
    octal(&mut block[108..116], 0); // uid
    octal(&mut block[116..124], 0); // gid
    octal(&mut block[124..136], size);
    octal(&mut block[136..148], 0); // mtime
    block[156] = b'0'; // regular file
    block[257..263].copy_from_slice(b"ustar\0");
    block[263..265].copy_from_slice(b"00");
    octal(&mut block[329..337], 0); // devmajor
    octal(&mut block[337..345], 0); // devminor
    block[345..345 + prefix.len()].copy_from_slice(prefix);
    // The checksum: the byte sum with the checksum field read as eight spaces, then six octal
    // digits, a NUL and a space.
    block[148..156].copy_from_slice(b"        ");
    let sum: u64 = block.iter().map(|b| u64::from(*b)).sum();
    octal(&mut block[148..155], sum);
    block[155] = b' ';
    block
}

/// The canonical ustar archive of a part's files: entries in byte-wise order of name, mode 0644,
/// owner 0, time 0, no user or group names, two zero blocks at the end, no compression.
pub fn canonical_archive(files: &PartFiles) -> Vec<u8> {
    let mut out = Vec::new();
    // `BTreeMap<String, _>` iterates in byte-wise order of its keys.
    for (name, content) in &files.0 {
        out.extend_from_slice(&header(name, content.len() as u64));
        out.extend_from_slice(content);
        let padding = (BLOCK - content.len() % BLOCK) % BLOCK;
        out.resize(out.len() + padding, 0);
    }
    out.resize(out.len() + 2 * BLOCK, 0);
    out
}

/// SHA-256 of the canonical archive.
pub fn content_digest(files: &PartFiles) -> Sha256Hex {
    let digest: [u8; 32] = Sha256::digest(canonical_archive(files)).into();
    Sha256Hex::from_digest(&digest)
}

/// `sha256:<hex>`, the spelling used inside the lock file and the withdrawn list.
pub fn prefixed_digest(d: &Sha256Hex) -> String {
    format!("{DIGEST_PREFIX}{}", d.0)
}

pub fn parse_prefixed_digest(s: &str) -> Result<Sha256Hex, AmError> {
    s.strip_prefix(DIGEST_PREFIX)
        .and_then(|hex| Sha256Hex::parse(hex).ok())
        .ok_or_else(|| AmError::DigestInvalid {
            value: s.to_owned(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn files(entries: &[(&str, &[u8])]) -> PartFiles {
        PartFiles(
            entries
                .iter()
                .map(|(n, c)| ((*n).to_owned(), c.to_vec()))
                .collect::<BTreeMap<_, _>>(),
        )
    }

    #[test]
    fn archive_layout() {
        let archive = canonical_archive(&files(&[("b.md", b"x"), ("a.md", b"hello")]));
        assert_eq!(archive.len(), 512 * 2 + 512 * 2 + 1024);
        assert_eq!(&archive[..4], b"a.md");
        assert_eq!(&archive[100..108], b"0000644\0");
        assert_eq!(&archive[124..136], b"00000000005\0");
        assert_eq!(&archive[257..265], b"ustar\x0000");
        assert_eq!(&archive[1024..1028], b"b.md");
        // The checksum verifies: recompute it with the field as spaces.
        let mut head = archive[..512].to_vec();
        let stored = std::str::from_utf8(&head[148..154]).unwrap().to_owned();
        head[148..156].copy_from_slice(b"        ");
        let sum: u64 = head.iter().map(|b| u64::from(*b)).sum();
        assert_eq!(u64::from_str_radix(&stored, 8).unwrap(), sum);
        assert!(archive[archive.len() - 1024..].iter().all(|b| *b == 0));
    }

    #[test]
    fn long_names_split() {
        let long = format!("{}/{}/{}", "d".repeat(70), "e".repeat(70), "f".repeat(90));
        let (prefix, name) = ustar_split(&long).unwrap();
        assert_eq!(prefix.len(), 141);
        assert_eq!(name.len(), 90);
        let archive = canonical_archive(&files(&[(long.as_str(), b"")]));
        assert_eq!(&archive[345..348], b"ddd");
        assert_eq!(ustar_split(&"x".repeat(101)), None);
        assert_eq!(
            ustar_split(&format!("{}/{}", "d".repeat(150), "f".repeat(50))),
            None
        );
        // 255 bytes in all is the limit even where a split exists.
        let parts = ["a".repeat(100), "b".repeat(54), "c".repeat(100)];
        assert_eq!(ustar_split(&parts.join("/")).map(|(p, _)| p.len()), None);
        assert_eq!(
            ustar_split(&format!("{}/{}", "d".repeat(150), "f".repeat(110))),
            None
        );
        assert_eq!(ustar_split(""), None);
    }

    #[test]
    fn digests() {
        let d = content_digest(&files(&[("SKILL.md", b"x")]));
        assert_eq!(d.0.len(), 64);
        assert_eq!(parse_prefixed_digest(&prefixed_digest(&d)).unwrap(), d);
        for bad in ["sha256:XYZ", "md5:00", &d.0] {
            assert_eq!(
                parse_prefixed_digest(bad).unwrap_err().code(),
                "AM_DIGEST_INVALID"
            );
        }
    }
}
