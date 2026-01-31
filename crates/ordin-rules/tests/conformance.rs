//! The rules-engine conformance suite (Playbook Step 27 gate; plan 01, Phase 4), driven from the
//! committed fixtures in `testdata/conformance/rules/`. Each fixture restates rule-evaluation cases
//! from RulesEngine.md in this engine's vocabulary and names them in its `source`. A fixture that
//! cannot be read or parsed fails the suite; an empty folder fails it too.

mod support;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use ordin_rules::{
    CallContext, ConfiguredControls, Declaration, DeliveryTo, ProgramKind, RulesEngine,
    StartOutcome, ToolDescription,
};
use ordin_types::domain::agents::Authority;
use ordin_types::wire::rules::{ToolOrigin, Verdict};
use ordin_types::{RunId, WorkspaceId};
use serde::Deserialize;
use support::{fixture, rule_set, running, start, Fixture};

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct SetFixture {
    #[serde(default)]
    tools: Vec<ToolDescription>,
    #[serde(default)]
    commands: Vec<String>,
    #[serde(default)]
    approval_list: Vec<String>,
    #[serde(default)]
    declaration: Option<Declaration>,
    #[serde(default)]
    configured: Option<ConfiguredControls>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CallCase {
    #[serde(default)]
    files: HashMap<String, String>,
    tool: String,
    arguments: serde_json::Value,
    verdict: String,
    #[serde(default)]
    rule: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StartCase {
    text: String,
    outcome: String,
    #[serde(default)]
    control: Option<String>,
    #[serde(default)]
    reply: Option<String>,
    #[serde(default)]
    actions_attached: Option<Vec<String>>,
    #[serde(default)]
    tool_subset: Option<Vec<String>>,
    #[serde(default)]
    single_call: Option<bool>,
    #[serde(default)]
    command: Option<String>,
    /// Present (even as null) = checked.
    #[serde(default, deserialize_with = "present")]
    delivery: Option<Option<DeliveryTo>>,
}

fn present<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Option<DeliveryTo>>, D::Error> {
    Option::<DeliveryTo>::deserialize(d).map(Some)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LoadCase {
    name: String,
    set: SetFixture,
    code: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AddressCase {
    text: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Suite {
    source: String,
    #[serde(default)]
    set: Option<SetFixture>,
    #[serde(default)]
    calls: Vec<CallCase>,
    #[serde(default)]
    starts: Vec<StartCase>,
    #[serde(default)]
    loads: Vec<LoadCase>,
    #[serde(default)]
    addresses: Vec<AddressCase>,
}

fn folder() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/conformance/rules")
}

fn suites() -> Vec<(String, Suite)> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(folder())
        .expect("testdata/conformance/rules/ is committed")
        .map(|e| e.expect("a folder entry").path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "the conformance folder holds no fixture");
    files
        .into_iter()
        .map(|p| {
            let text = std::fs::read_to_string(&p).expect("a readable fixture");
            let suite: Suite = serde_json::from_str(&text)
                .unwrap_or_else(|e| panic!("{} does not parse: {e}", p.display()));
            (p.file_name().unwrap().to_string_lossy().into_owned(), suite)
        })
        .collect()
}

fn build(f: &Fixture, s: SetFixture) -> ordin_rules::InstanceRuleSet {
    let mut set = rule_set(Authority::DocumentOnly, &f.folder("conformance"));
    set.tools = s.tools;
    set.commands = s.commands;
    set.approval_list = s.approval_list;
    set.declaration = s.declaration;
    set.configured = s.configured;
    set
}

fn verdict(v: &Verdict) -> &'static str {
    match v {
        Verdict::Allow => "allow",
        Verdict::Deny { .. } => "deny",
        Verdict::Ask { .. } => "ask",
    }
}

async fn run_calls(file: &str, set: SetFixture, calls: Vec<CallCase>) {
    let f = fixture(ProgramKind::Desktop).await;
    let r = running(&f, build(&f, set), None).await;
    for (i, case) in calls.into_iter().enumerate() {
        f.host.files.lock().unwrap().extend(case.files);
        let call = r.call(
            &case.tool,
            ToolOrigin::OrdinMcpFront {
                server: "work".to_owned(),
            },
            case.arguments.clone(),
        );
        let outcome = f
            .engine
            .decide_tool_call(&call, &CallContext::default())
            .await;
        let at = format!("{file} calls[{i}] {} {}", case.tool, case.arguments);
        assert_eq!(
            verdict(&outcome.decision.verdict),
            case.verdict,
            "{at}: {outcome:?}"
        );
        if let Some(rule) = case.rule {
            assert_eq!(outcome.decision.rule.rule_id, rule, "{at}");
        }
    }
}

async fn run_starts(file: &str, set: SetFixture, starts: Vec<StartCase>) {
    let f = fixture(ProgramKind::Desktop).await;
    let set = build(&f, set);
    let agent = set.agent_instance_id;
    f.engine.load_rule_set(set).expect("the set loads");
    for (i, case) in starts.into_iter().enumerate() {
        let at = format!("{file} starts[{i}] {:?}", case.text);
        let mut facts = start(RunId::new(), agent, WorkspaceId::new());
        facts.text = case.text.clone();
        let decision = f.engine.begin_run(facts).await.expect("the run begins");
        match (&decision.outcome, case.outcome.as_str()) {
            (StartOutcome::Proceed, "proceed") => {}
            (StartOutcome::Replied { control, text }, "replied") => {
                if let Some(expected) = &case.control {
                    assert_eq!(control, expected, "{at}");
                }
                if let Some(expected) = &case.reply {
                    assert_eq!(text, expected, "{at}");
                }
            }
            (StartOutcome::Ended { control, .. }, "ended") => {
                if let Some(expected) = &case.control {
                    assert_eq!(control, expected, "{at}");
                }
            }
            (got, want) => panic!("{at}: expected {want}, got {got:?}"),
        }
        if let Some(actions) = &case.actions_attached {
            assert_eq!(&decision.actions_attached, actions, "{at}");
        }
        if let Some(subset) = &case.tool_subset {
            assert_eq!(decision.tool_subset.as_ref(), Some(subset), "{at}");
        }
        if let Some(single) = case.single_call {
            assert_eq!(decision.single_call, single, "{at}");
        }
        if let Some(command) = &case.command {
            assert_eq!(decision.command.as_ref(), Some(command), "{at}");
        }
        if let Some(delivery) = case.delivery {
            assert_eq!(decision.delivery, delivery, "{at}");
        }
    }
}

#[tokio::test]
async fn the_conformance_fixtures_produce_the_expected_verdicts() {
    let mut cases = 0;
    for (file, suite) in suites() {
        assert!(!suite.source.is_empty(), "{file} names its source");
        cases += suite.calls.len() + suite.starts.len() + suite.loads.len() + suite.addresses.len();
        if !suite.calls.is_empty() || !suite.starts.is_empty() {
            let set = suite
                .set
                .unwrap_or_else(|| panic!("{file} has cases and no set"));
            if suite.starts.is_empty() {
                run_calls(&file, set, suite.calls).await;
            } else if suite.calls.is_empty() {
                run_starts(&file, set, suite.starts).await;
            } else {
                panic!("{file}: one fixture holds calls or starts, not both");
            }
        }
        for load in suite.loads {
            let f = fixture(ProgramKind::Desktop).await;
            let error = f
                .engine
                .load_rule_set(build(&f, load.set))
                .expect_err(&format!("{file} loads {:?} is refused", load.name));
            assert_eq!(error.code(), load.code, "{file} loads {:?}", load.name);
        }
        for address in suite.addresses {
            let error = RulesEngine::parse_address(&address.text)
                .expect_err(&format!("{file}: {:?} is refused", address.text));
            assert_eq!(error.code(), "RULE_NOT_AN_ADDRESS", "{:?}", address.text);
        }
    }
    assert!(cases >= 40, "the suite runs {cases} cases");
}
