//! The SharedTypes.md scenarios whose last step names this crate: each re-export is the
//! `ordin-types` type itself, not a copy (compile-time identity, then `TypeId`). The exact
//! scenario title is the comment above each test.

use std::any::TypeId;

/// Compiles only when both arguments have one type.
fn same_type<T>(_: &T, _: &T) {}

// Scenario: Happy path — a model choice serialises in the 2C form
// (And ordin_runtime_adapter::ModelChoice is the same type as ordin_types::wire::run::ModelChoice)
#[test]
fn happy_path_a_model_choice_serialises_in_the_2c_form() {
    let ours = ordin_runtime_adapter::ModelChoice {
        level: ordin_runtime_adapter::ModelLevel::Standard,
        model: Some("sonnet".into()),
    };
    let theirs: ordin_types::wire::run::ModelChoice = ours.clone();
    same_type(&ours, &theirs);
    assert_eq!(
        TypeId::of::<ordin_runtime_adapter::ModelChoice>(),
        TypeId::of::<ordin_types::wire::run::ModelChoice>()
    );
    assert_eq!(
        serde_json::to_string(&ours).unwrap(),
        r#"{"level":"standard","model":"sonnet"}"#
    );
}

// Scenario: Happy path — a crew member's tool caller serialises in the 2C form
// (And ordin_runtime_adapter::ToolCaller is the same type as ordin_types::session::ToolCaller)
#[test]
fn happy_path_a_crew_members_tool_caller_serialises_in_the_2c_form() {
    let ours = ordin_runtime_adapter::ToolCaller::CrewMember {
        crew_run_id: ordin_types::CrewRunId::parse("0192a1c5-0000-7000-8000-000000000001").unwrap(),
    };
    let theirs: ordin_types::session::ToolCaller = ours;
    same_type(&ours, &theirs);
    assert_eq!(
        TypeId::of::<ordin_runtime_adapter::ToolCaller>(),
        TypeId::of::<ordin_types::session::ToolCaller>()
    );
    assert_eq!(
        serde_json::to_string(&ours).unwrap(),
        r#"{"kind":"crew_member","crew_run_id":"0192a1c5-0000-7000-8000-000000000001"}"#
    );
}

// Scenario: Happy path — one AdapterKind across domain and wire
// (And ordin_runtime_adapter::AdapterKind is the same type as ordin_types::domain::runs::AdapterKind)
#[test]
fn happy_path_one_adapterkind_across_domain_and_wire() {
    let ours = ordin_runtime_adapter::AdapterKind::HostedAgent;
    let domain: ordin_types::domain::runs::AdapterKind = ours;
    let wire: ordin_types::wire::run::AdapterKind = ours;
    same_type(&ours, &domain);
    same_type(&ours, &wire);
    assert_eq!(
        TypeId::of::<ordin_runtime_adapter::AdapterKind>(),
        TypeId::of::<ordin_types::domain::runs::AdapterKind>()
    );
    assert_eq!(serde_json::to_string(&ours).unwrap(), r#""hosted_agent""#);
}

#[test]
fn every_re_export_is_the_ordin_types_type() {
    use ordin_runtime_adapter as ra;
    use ordin_types::wire::run as wire;
    let pairs = [
        (
            TypeId::of::<ra::AdapterVerb>(),
            TypeId::of::<wire::AdapterVerb>(),
        ),
        (
            TypeId::of::<ra::RunTarget>(),
            TypeId::of::<wire::RunTarget>(),
        ),
        (
            TypeId::of::<ra::RunLimits>(),
            TypeId::of::<wire::RunLimits>(),
        ),
        (
            TypeId::of::<ra::RunProgress>(),
            TypeId::of::<wire::RunProgress>(),
        ),
        (
            TypeId::of::<ra::ProgressCount>(),
            TypeId::of::<wire::ProgressCount>(),
        ),
        (
            TypeId::of::<ra::ModelLevel>(),
            TypeId::of::<wire::ModelLevel>(),
        ),
        (
            TypeId::of::<ra::ThinkingLevel>(),
            TypeId::of::<wire::ThinkingLevel>(),
        ),
        (
            TypeId::of::<ra::RunState>(),
            TypeId::of::<ordin_types::domain::runs::RunState>(),
        ),
        (
            TypeId::of::<ra::HarnessKind>(),
            TypeId::of::<ordin_types::domain::identity::HarnessKind>(),
        ),
    ];
    for (ours, theirs) in pairs {
        assert_eq!(ours, theirs);
    }
}
