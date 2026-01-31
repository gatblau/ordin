//! Lifecycles read from Journey parts (CatalogueService.md §F, with the ADR-102 and ADR-106
//! amendments): field mapping, the `LIFE_` rules, and the handover places the stages name.

use std::collections::BTreeSet;

use ordin_agent_model::{JourneyBody, JourneyStage, Manifest};
use ordin_types::domain::agents::{
    LifecycleStage, SendBack, StageChoice, StageNext, StageSlackChannel,
};
use ordin_types::domain::handover::HandoverPlaceType;
use ordin_types::HandoverPlaceId;

/// A stage before its place has an identifier (the identifier is minted in the ingest transaction).
#[derive(Debug, Clone)]
pub(crate) struct StageDraft {
    pub stage: LifecycleStage,
    pub place: (HandoverPlaceType, String),
}

/// A note for the health report of a lifecycle that stays offered or is withheld.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LifeNote {
    SlackChannelIgnored { stage: String },
    PlaceInvalid { stage: String },
    SeamLoop,
}

#[derive(Debug, Clone)]
pub(crate) struct LifecycleDraft {
    pub stages: Vec<StageDraft>,
    pub notes: Vec<LifeNote>,
}

/// The outcome of reading one Journey part.
#[derive(Debug, Clone)]
pub(crate) enum Reading {
    /// The part holds neither `seams` nor `stages`: it is not a lifecycle.
    NotALifecycle,
    Offered(LifecycleDraft),
    /// The lifecycle is withheld; `code` is the rule or Agent Model code.
    NotOffered {
        code: &'static str,
        notes: Vec<LifeNote>,
    },
}

const MAX_STAGES: usize = 30;

/// `^[CG][A-Z0-9]{8,20}$`, written out so the module needs no regular expression.
fn slack_id_valid(id: &str) -> bool {
    let bytes = id.as_bytes();
    (9..=21).contains(&bytes.len())
        && matches!(bytes.first(), Some(b'C' | b'G'))
        && bytes[1..]
            .iter()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
}

fn slack_name_valid(name: &str) -> bool {
    let chars = name.chars().count();
    (1..=80).contains(&chars) && !name.starts_with('#')
}

/// Jira Cloud only: an `https` address of at most 2,048 bytes whose host ends `.atlassian.net`
/// (A-53).
// spec: ordin-desktop/components/CatalogueService#ADR-106
pub(crate) fn jira_target_valid(target: &str) -> bool {
    if target.len() > 2048 {
        return false;
    }
    match url::Url::parse(target) {
        Ok(url) => {
            url.scheme() == "https"
                && url
                    .host_str()
                    .is_some_and(|host| host.to_ascii_lowercase().ends_with(".atlassian.net"))
        }
        Err(_) => false,
    }
}

fn place_of(stage: &JourneyStage) -> Option<(HandoverPlaceType, String)> {
    let target = stage.handover_place_target.clone();
    match stage.handover_place_type.as_str() {
        "git" => Some((HandoverPlaceType::Git, target)),
        "sharepoint" => Some((HandoverPlaceType::Sharepoint, target)),
        "jira" if jira_target_valid(&target) => Some((HandoverPlaceType::Jira, target)),
        _ => None,
    }
}

fn index_of(names: &[String], name: &str) -> Option<usize> {
    names.iter().position(|n| n == name)
}

/// Reads one Journey part's manifest (§F).
// spec: ordin-desktop/components/CatalogueService#ADR-092
// spec: ordin-desktop/components/CatalogueService#ADR-099
pub(crate) fn read_journey(manifest: &Manifest, lifecycle_name: &str, identifier: &str) -> Reading {
    let body = match manifest.journey() {
        Ok(Some(body)) => body,
        Ok(None) => return Reading::NotALifecycle,
        Err(error) => {
            return Reading::NotOffered {
                code: error.code(),
                notes: Vec::new(),
            }
        }
    };
    if body.stages.is_empty() {
        return read_seams(&body, identifier);
    }
    read_stages(&body, lifecycle_name)
}

fn read_stages(body: &JourneyBody, lifecycle_name: &str) -> Reading {
    let stages = &body.stages;
    let withheld = |code: &'static str, notes: Vec<LifeNote>| Reading::NotOffered { code, notes };
    let names: Vec<String> = stages.iter().map(|s| s.id.clone()).collect();
    let distinct: BTreeSet<&String> = names.iter().collect();
    if stages.is_empty()
        || stages.len() > MAX_STAGES
        || distinct.len() != names.len()
        || names.iter().any(|n| n.is_empty() || n.chars().count() > 80)
    {
        return withheld("LIFE_STAGES_INVALID", Vec::new());
    }
    let mut notes = Vec::new();
    let mut drafts = Vec::with_capacity(stages.len());
    for (at, stage) in stages.iter().enumerate() {
        let role_chars = stage.role.chars().count();
        if role_chars == 0 || role_chars > 200 {
            return withheld("LIFE_ROLE_INVALID", notes);
        }
        let days = stage.answer_within_days.unwrap_or(0);
        if days > 365 {
            return withheld("LIFE_DAYS_OUT_OF_RANGE", notes);
        }
        let Some(place) = place_of(stage) else {
            notes.push(LifeNote::PlaceInvalid {
                stage: stage.id.clone(),
            });
            return withheld("LIFE_PLACE_INVALID", notes);
        };
        let next = match &stage.next {
            None => match stages.get(at + 1) {
                Some(following) => StageNext::One {
                    stage: following.id.clone(),
                },
                None => StageNext::Nothing,
            },
            Some(entries) if entries.is_empty() => StageNext::Nothing,
            Some(entries) if entries.len() == 1 => StageNext::One {
                stage: entries[0].stage.clone(),
            },
            Some(entries) if stage.together => StageNext::SideBySide {
                stages: entries.iter().map(|e| e.stage.clone()).collect(),
            },
            Some(entries) => {
                if !(2..=4).contains(&entries.len()) {
                    return withheld("LIFE_CHOICES_INVALID", notes);
                }
                StageNext::OneOf {
                    choices: entries
                        .iter()
                        .map(|e| StageChoice {
                            stage: e.stage.clone(),
                            when: e.when.clone().unwrap_or_default(),
                        })
                        .collect(),
                }
            }
        };
        let targets: Vec<&String> = match &next {
            StageNext::One { stage } => vec![stage],
            StageNext::OneOf { choices } => choices.iter().map(|c| &c.stage).collect(),
            StageNext::SideBySide { stages } => stages.iter().collect(),
            StageNext::Nothing => Vec::new(),
        };
        for target in targets {
            match index_of(&names, target) {
                Some(to) if to > at => {}
                Some(_) => return withheld("LIFE_STAGE_BACKWARD", notes),
                None => return withheld("LIFE_STAGES_INVALID", notes),
            }
        }
        let mut send_backs = Vec::new();
        for back in &stage.sends_back_to {
            match index_of(&names, &back.stage) {
                Some(to) if to < at => {}
                _ => return withheld("LIFE_SEND_BACK_NOT_EARLIER", notes),
            }
            if !(1..=10).contains(&back.max_rounds) {
                return withheld("LIFE_SEND_BACK_ROUNDS_OUT_OF_RANGE", notes);
            }
            send_backs.push(SendBack {
                to_stage: back.stage.clone(),
                when: back.when.clone(),
                max_rounds: back.max_rounds,
            });
        }
        // ADR-102: a malformed channel is dropped, the lifecycle stays offered.
        let slack_channel = match &stage.slack_channel {
            Some(channel) if slack_id_valid(&channel.id) && slack_name_valid(&channel.name) => {
                Some(StageSlackChannel {
                    channel_id: channel.id.clone(),
                    name: channel.name.clone(),
                })
            }
            Some(_) => {
                notes.push(LifeNote::SlackChannelIgnored {
                    stage: stage.id.clone(),
                });
                None
            }
            None => None,
        };
        drafts.push(StageDraft {
            stage: LifecycleStage {
                name: stage.id.clone(),
                role: stage.role.clone(),
                agent: stage.agent.clone(),
                hands_over: stage.produces.clone(),
                handover_place_id: HandoverPlaceId(uuid::Uuid::nil()),
                days_to_answer: days,
                next,
                send_backs,
                slack_channel,
            },
            place,
        });
    }
    // Every stage is reachable from the first; `next` only leads forward, so one sweep decides.
    let mut reached = vec![false; drafts.len()];
    reached[0] = true;
    for at in 0..drafts.len() {
        if !reached[at] {
            return withheld("LIFE_STAGE_UNREACHABLE", notes);
        }
        let targets: Vec<String> = match &drafts[at].stage.next {
            StageNext::One { stage } => vec![stage.clone()],
            StageNext::OneOf { choices } => choices.iter().map(|c| c.stage.clone()).collect(),
            StageNext::SideBySide { stages } => stages.clone(),
            StageNext::Nothing => Vec::new(),
        };
        for target in targets {
            if let Some(to) = index_of(&names, &target) {
                reached[to] = true;
            }
        }
    }
    let _ = lifecycle_name;
    Reading::Offered(LifecycleDraft {
        stages: drafts,
        notes,
    })
}

// A journey without `stages` is read through its seams (spec §F): a seam written
// `<producer>-to-<consumer>` is an edge between two stages named by those words, a seam without that
// form is one stage, each stage's role is its name, and every stage has the Git place
// `seams:<journey identifier>`.
fn read_seams(body: &JourneyBody, identifier: &str) -> Reading {
    if body.seams.is_empty() {
        return Reading::NotOffered {
            code: "LIFE_STAGES_INVALID",
            notes: Vec::new(),
        };
    }
    let mut nodes: Vec<String> = Vec::new();
    let mut edges: Vec<(usize, usize)> = Vec::new();
    let node = |name: &str, nodes: &mut Vec<String>| match index_of(nodes, name) {
        Some(at) => at,
        None => {
            nodes.push(name.to_owned());
            nodes.len() - 1
        }
    };
    for seam in &body.seams {
        match seam.split_once("-to-") {
            Some((producer, consumer)) if !producer.is_empty() && !consumer.is_empty() => {
                let from = node(producer, &mut nodes);
                let to = node(consumer, &mut nodes);
                edges.push((from, to));
            }
            _ => {
                node(seam, &mut nodes);
            }
        }
    }
    if nodes.len() > MAX_STAGES {
        return Reading::NotOffered {
            code: "LIFE_STAGES_INVALID",
            notes: Vec::new(),
        };
    }
    // Kahn's algorithm, ties broken by first appearance; a leftover node means a loop.
    let mut indegree = vec![0usize; nodes.len()];
    for (_, to) in &edges {
        indegree[*to] += 1;
    }
    let mut order: Vec<usize> = Vec::new();
    let mut ready: Vec<usize> = (0..nodes.len()).filter(|n| indegree[*n] == 0).collect();
    while let Some(next) = ready.first().copied() {
        ready.remove(0);
        order.push(next);
        for (from, to) in edges.clone() {
            if from == next {
                indegree[to] -= 1;
                if indegree[to] == 0 {
                    ready.push(to);
                    ready.sort_unstable();
                }
            }
        }
    }
    if order.len() != nodes.len() {
        return Reading::NotOffered {
            code: "JOURNEY_SEAMS_LOOP",
            notes: vec![LifeNote::SeamLoop],
        };
    }
    let place = (HandoverPlaceType::Git, format!("seams:{identifier}"));
    let stages = order
        .iter()
        .map(|&n| {
            let mut successors: Vec<String> = edges
                .iter()
                .filter(|(from, _)| *from == n)
                .map(|(_, to)| nodes[*to].clone())
                .collect();
            successors.dedup();
            let next = match successors.len() {
                0 => StageNext::Nothing,
                1 => StageNext::One {
                    stage: successors.remove(0),
                },
                _ => StageNext::SideBySide { stages: successors },
            };
            StageDraft {
                stage: LifecycleStage {
                    name: nodes[n].clone(),
                    role: nodes[n].clone(),
                    agent: None,
                    hands_over: None,
                    handover_place_id: HandoverPlaceId(uuid::Uuid::nil()),
                    days_to_answer: 0,
                    next,
                    send_backs: Vec::new(),
                    slack_channel: None,
                },
                place: place.clone(),
            }
        })
        .collect();
    Reading::Offered(LifecycleDraft {
        stages,
        notes: Vec::new(),
    })
}
