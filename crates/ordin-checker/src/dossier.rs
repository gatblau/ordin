//! Steps 36–38: the dossier reader over an OutputContract's sections. The write-up is data: no link is followed and nothing is executed.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use ordin_agent_model::{ContractSection, ModelPart, OutputContract, PartKind};
use regex::Regex;

use crate::error::CheckError;
use crate::model::{DossierReading, DossierSection};

static HEADING: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"^#{1,6}[ \t]*(.*)$").ok());
static ITEM: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"^[ \t]*([-*]|[0-9]+\.)").ok());

/// Lower case, trimmed, each run outside `[a-z0-9]` as `-`, `-` trimmed.
pub(crate) fn normalise(text: &str) -> String {
    let lower = text.trim().to_lowercase();
    let mut out = String::with_capacity(lower.len());
    let mut in_run = false;
    for c in lower.chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            out.push(c);
            in_run = false;
        } else if !in_run {
            out.push('-');
            in_run = true;
        }
    }
    out.trim_matches('-').to_owned()
}

/// Step 38: the contract's sections through the Agent Model's reader.
// spec: ordin-desktop/components/Checker#SA-CLOSE-02
pub(crate) fn sections(contract: &ModelPart) -> Result<Vec<ContractSection>, CheckError> {
    OutputContract::read(&contract.manifest)
        .map(|c| c.sections().to_vec())
        .map_err(|_| CheckError::ContractInvalid {
            reason: "section_malformed",
        })
}

#[derive(Default)]
struct Block {
    text: String,
    evidence: u32,
}

/// Steps 36–37.
pub(crate) fn read(write_up: &str, contract: &ModelPart) -> Result<DossierReading, CheckError> {
    if contract.kind != PartKind::OutputContract {
        return Err(CheckError::ContractInvalid {
            reason: "not_output_contract",
        });
    }
    let sections = sections(contract)?;
    if sections.is_empty() {
        return Err(CheckError::ContractInvalid {
            reason: "no_sections",
        });
    }
    let names: BTreeMap<String, usize> = sections
        .iter()
        .enumerate()
        .map(|(i, s)| (normalise(&s.name), i))
        .collect();
    let mut blocks: BTreeMap<usize, Block> = BTreeMap::new();
    let mut unmatched: Vec<String> = Vec::new();
    let mut current: Option<usize> = None;
    for line in write_up.lines() {
        let heading = HEADING
            .as_ref()
            .and_then(|re| re.captures(line))
            .map(|caps| caps.get(1).map_or("", |m| m.as_str()).to_owned());
        if let Some(heading) = heading {
            let name = normalise(&heading);
            match names.get(&name) {
                Some(index) => {
                    blocks.entry(*index).or_default();
                    current = Some(*index);
                }
                None => {
                    if !unmatched.contains(&name) {
                        unmatched.push(name);
                    }
                    current = None;
                }
            }
            continue;
        }
        let Some(index) = current else { continue };
        let block = blocks.entry(index).or_default();
        if !block.text.is_empty() {
            block.text.push('\n');
        }
        block.text.push_str(line);
        if ITEM.as_ref().is_some_and(|re| re.is_match(line)) {
            block.evidence += 1;
        }
    }
    let mut reading = DossierReading {
        sections: Vec::new(),
        missing_sections: Vec::new(),
        unmatched_headings: unmatched,
    };
    for (index, section) in sections.iter().enumerate() {
        match blocks.remove(&index) {
            None => reading.missing_sections.push(section.name.clone()),
            Some(block) => {
                let text = block.text.trim().to_owned();
                let evidence_count = if block.evidence == 0 && !text.is_empty() {
                    1
                } else {
                    block.evidence
                };
                reading.sections.push(DossierSection {
                    name: section.name.clone(),
                    low_confidence: section.required && evidence_count < section.evidence_threshold,
                    text,
                    evidence_count,
                    required: section.required,
                    derived: section.derived,
                });
            }
        }
    }
    Ok(reading)
}
