//! X2: one test per Gherkin scenario of CatalogueService.md, named after it.

mod support;

use ordin_agent_model::{PartKind, PartSource, RepositoryRef, Resolution, SourceScope};
use ordin_catalogue::{
    AddRepositoryInput, Coverage, FetchResult, HealthLine, LibraryQuery, MatchRequirementsInput,
    PartStateLabel, ProbeRepositoryInput, RepositorySyncOutcome,
};
use ordin_projects::{GithubError, RepoPermission};
use ordin_types::domain::agents::StageNext;
use ordin_types::domain::agents::{AgentFormat, AgentRepositoryState, RepositorySource};
use support::*;

#[tokio::test(flavor = "multi_thread")]
async fn happy_path_add_an_agent_repository_the_person_can_read() {
    let env = Env::new(&[]).await;
    let owner = unique_owner();
    *env.github.permission.lock().unwrap() = RepoPermission::Read;
    // The APM package is read and added: format, count and rights are reported, and the row exists.
    commit_tree(
        &owner,
        "apm-agents",
        &testdata_tree("parity/format-plugins/apm-basic"),
    );
    let probe = env
        .service
        .probe_repository(ProbeRepositoryInput {
            web_address: format!("https://github.com/{owner}/apm-agents"),
        })
        .await
        .expect("the probe");
    assert_eq!(probe.format, AgentFormat::Apm);
    assert_eq!(probe.agent_count, 1);
    assert!(!probe.person_can_publish);
    let added = env
        .service
        .add_repository(AddRepositoryInput {
            probe_id: probe.probe_id,
            display_name: "Imported agents".into(),
        })
        .await
        .expect("the add");
    assert_eq!(added.source, RepositorySource::Person);
    assert_eq!(added.state, AgentRepositoryState::Cloned);
    // An agent the Checker accepts is listed with the repository's name. (An imported APM agent carries
    // no category or audience, so the Checker does not offer it: see FU-C1 of the phase report.)
    commit_tree(&owner, "finance-agents", &agents_repo(&["rate-checker"]));
    let probe = env
        .service
        .probe_repository(ProbeRepositoryInput {
            web_address: format!("https://github.com/{owner}/finance-agents.git"),
        })
        .await
        .expect("the probe");
    assert_eq!(probe.format, AgentFormat::Ordin);
    assert_eq!(probe.agent_count, 1);
    assert_eq!(probe.suggested_name, "finance-agents");
    let added = env
        .service
        .add_repository(AddRepositoryInput {
            probe_id: probe.probe_id,
            display_name: "Finance agents".into(),
        })
        .await
        .expect("the add");
    assert_eq!(added.state, AgentRepositoryState::Cloned);
    let page = env
        .service
        .search(LibraryQuery::default())
        .await
        .expect("the library");
    assert_eq!(
        page.items.len(),
        1,
        "{:?}",
        page.items
            .iter()
            .map(|i| &i.part.part.identifier)
            .collect::<Vec<_>>()
    );
    assert_eq!(page.items[0].from, "Finance agents");
    assert_eq!(page.items[0].part.state_label, PartStateLabel::Current);
}

#[tokio::test(flavor = "multi_thread")]
async fn edge_adding_turned_off_by_the_company() {
    let env = Env::with(&[], false).await;
    let error = env
        .service
        .probe_repository(ProbeRepositoryInput {
            web_address: "https://github.com/acme/anything".into(),
        })
        .await
        .unwrap_err();
    assert_eq!(error.code, "AREPO_ADD_DISABLED");
    assert_eq!(error.action, None);
}

#[tokio::test(flavor = "multi_thread")]
async fn error_no_read_access() {
    let env = Env::new(&[]).await;
    *env.github.permission.lock().unwrap() = RepoPermission::None;
    let error = env
        .service
        .probe_repository(ProbeRepositoryInput {
            web_address: "https://github.com/northwind/secret-agents".into(),
        })
        .await
        .unwrap_err();
    assert_eq!(error.code, "CAT_NO_READ_ACCESS");
    assert_eq!(
        env.count("SELECT COUNT(*) FROM agent_repositories").await,
        0
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn ingest_is_all_or_nothing_per_commit() {
    let owner = unique_owner();
    let env = Env::new(&names(&owner, &["agents"])).await;
    let commit_a = commit_tree(&owner, "agents", &agents_repo(&["first"]));
    assert_eq!(sync_one(&env).await, RepositorySyncOutcome::Cloned);
    // The write of commit B fails midway: every dependency insert is refused.
    sqlx::query("CREATE TRIGGER refuse_edges BEFORE INSERT ON part_dependencies BEGIN SELECT RAISE(ABORT, 'disk full'); END")
        .execute(env.pool())
        .await
        .unwrap();
    commit_tree(&owner, "agents", &agents_repo(&["first", "second"]));
    let outcomes = env.service.fetch_now(None).await.unwrap();
    assert_eq!(
        outcomes[0].result,
        FetchResult::CommitRefused {
            reason_code: "CAT_STORE_FAILED".into()
        }
    );
    let repo = repo_id(&env, "agents").await;
    let row = &env.service.list_repositories().await.unwrap()[0];
    assert_eq!(
        row.repository.last_fetched_commit.as_deref(),
        Some(commit_a.as_str())
    );
    let page = env.service.search(LibraryQuery::default()).await.unwrap();
    let names: Vec<_> = page
        .items
        .iter()
        .map(|i| i.part.part.name.clone())
        .collect();
    assert_eq!(names, ["first"], "no part of commit B is visible: {repo}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_part_that_fails_its_checks_is_not_offered() {
    let owner = unique_owner();
    let env = Env::new(&names(&owner, &["agents"])).await;
    let skill = markdown(
        "skill",
        "urn:ordin:acme:skills:summarise",
        "Summarise",
        "  category: nosuchterm\n",
        "Summarise the quote.\n",
    );
    let mut files = agents_repo(&["clean"]);
    let (path, bytes) = agent_file("uses-skill", "Uses a skill. It writes a summary.");
    let text = String::from_utf8(bytes).unwrap().replace(
        "defaultRules:",
        "skills:\n  - urn:ordin:acme:skills:summarise\ndefaultRules:",
    );
    files.insert(path, text.into_bytes());
    files.insert(
        "models/@acme/skills/summarise/SKILL.md".into(),
        skill.into_bytes(),
    );
    commit_tree(&owner, "agents", &files);
    sync_one(&env).await;
    let offered = |id: &'static str| {
        let env = &env;
        async move {
            sqlx::query_scalar::<_, i64>("SELECT i.offered FROM parts p JOIN part_index i ON i.part_id = p.id WHERE p.identifier = ?1")
                .bind(id)
                .fetch_one(env.pool())
                .await
                .unwrap()
        }
    };
    assert_eq!(offered("urn:ordin:acme:skills:summarise").await, 0);
    assert_eq!(offered("urn:ordin:acme:workspaces:uses-skill").await, 0);
    assert_eq!(offered("urn:ordin:acme:workspaces:clean").await, 1);
    assert_eq!(offered(TONE_ID).await, 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn unsigned_merge_where_signing_is_required() {
    let owner = unique_owner();
    let mut entry = Entry::ordin("agents", &owner, "agents");
    entry.signed = true;
    let env = Env::new(&[entry]).await;
    commit_tree(&owner, "agents", &agents_repo(&["first"]));
    *env.github.verified.lock().unwrap() = false;
    assert_eq!(sync_one(&env).await, RepositorySyncOutcome::Cloned);
    assert_eq!(
        last_result(&env, "agents").await,
        Some(FetchResult::MergeUnsigned)
    );
    let row = &env.service.list_repositories().await.unwrap()[0];
    assert_eq!(row.repository.last_fetched_commit, None);
    assert!(env
        .service
        .search(LibraryQuery::default())
        .await
        .unwrap()
        .items
        .is_empty());
}

fn withdrawn_yaml(version: &str, digest: &str) -> String {
    format!(
        "listSchema: ordin.withdrawn/v1\nentries:\n  - part: {{ kind: Skill, id: \"urn:ordin:acme:skills:rate-compare\", version: \"{version}\", contentHash: \"{digest}\" }}\n    reason: \"Uses the retired rates feed\"\n    withdrawnAt: \"2026-01-10T09:00:00.000Z\"\n"
    )
}

const DIGEST: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn rate_compare_skill() -> (String, String) {
    (
        "models/@acme/skills/rate-compare/SKILL.md".to_owned(),
        markdown(
            "skill",
            "urn:ordin:acme:skills:rate-compare",
            "Rate compare",
            "  version: 1.2.0\n",
            "Compare day rates across quotes.\n",
        )
        .replace("  version: 1.0.0\n", ""),
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn withdrawn_list_that_cannot_be_read_fails_closed() {
    let owner = unique_owner();
    let env = Env::new(&names(&owner, &["agents"])).await;
    let (path, text) = rate_compare_skill();
    let good = with(
        with(agents_repo(&["first"]), &path, &text),
        "withdrawn.yaml",
        &withdrawn_yaml("1.2.0", DIGEST),
    );
    commit_tree(&owner, "agents", &good);
    sync_one(&env).await;
    let id = repo_id(&env, "agents").await;
    let before = env.service.withdrawn_list(id).await.unwrap();
    assert_eq!(before.len(), 1);
    let bad = with(
        with(agents_repo(&["first", "second"]), &path, &text),
        "withdrawn.yaml",
        "listSchema: other/v9\nentries: []\n",
    );
    commit_tree(&owner, "agents", &bad);
    let outcomes = env.service.fetch_now(None).await.unwrap();
    assert_eq!(outcomes[0].result, FetchResult::WithdrawnListUnreadable);
    let after = env.service.withdrawn_list(id).await.unwrap();
    assert_eq!(after.len(), 1, "the last list read stays in force");
    assert_eq!(after[0].id, before[0].id);
    let names: Vec<_> = env
        .service
        .search(LibraryQuery::default())
        .await
        .unwrap()
        .items
        .iter()
        .map(|i| i.part.part.name.clone())
        .collect();
    assert_eq!(
        names,
        ["first"],
        "no update from that repository is ingested"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_entry_removed_by_hand_stays_in_force() {
    let owner = unique_owner();
    let env = Env::new(&names(&owner, &["agents"])).await;
    let (path, text) = rate_compare_skill();
    commit_tree(
        &owner,
        "agents",
        &with(
            with(agents_repo(&["first"]), &path, &text),
            "withdrawn.yaml",
            &withdrawn_yaml("1.2.0", DIGEST),
        ),
    );
    sync_one(&env).await;
    let id = repo_id(&env, "agents").await;
    // The new list no longer holds the entry.
    commit_tree(
        &owner,
        "agents",
        &with(
            with(agents_repo(&["first"]), &path, &text),
            "withdrawn.yaml",
            "listSchema: ordin.withdrawn/v1\nentries: []\n",
        ),
    );
    let outcomes = env.service.fetch_now(None).await.unwrap();
    assert!(matches!(outcomes[0].result, FetchResult::Ingested { .. }));
    let list = env.service.withdrawn_list(id).await.unwrap();
    assert_eq!(list.len(), 1);
    assert!(list[0].removed_by_hand_seen_at.is_some());
    let view = env
        .service
        .find_part(
            id,
            PartKind::Skill,
            "urn:ordin:acme:skills:rate-compare",
            Some("1.2.0"),
        )
        .await
        .unwrap();
    assert!(view.part.withdrawn);
    assert_eq!(view.state_label, PartStateLabel::Withdrawn);
}

async fn library_with_rate_compare() -> (Env, String) {
    let owner = unique_owner();
    let env = Env::new(&names(&owner, &["agents"])).await;
    let (path, text) = rate_compare_skill();
    commit_tree(
        &owner,
        "agents",
        &with(agents_repo(&["first"]), &path, &text),
    );
    sync_one(&env).await;
    (env, owner)
}

#[tokio::test(flavor = "multi_thread")]
async fn requirement_matching_by_identity() {
    let (env, _) = library_with_rate_compare().await;
    let matches = env
        .service
        .match_requirements(MatchRequirementsInput {
            requirements: vec!["Use urn:ordin:acme:skills:rate-compare on the quotes".into()],
        })
        .await
        .unwrap();
    assert_eq!(matches[0].coverage, Coverage::Covered);
    assert_eq!(
        matches[0].matched[0].part.identifier,
        "urn:ordin:acme:skills:rate-compare"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn empty_requirement_text() {
    let (env, _) = library_with_rate_compare().await;
    let matches = env
        .service
        .match_requirements(MatchRequirementsInput {
            requirements: vec!["   ".into()],
        })
        .await
        .unwrap();
    assert_eq!(matches[0].coverage, Coverage::NotCovered);
    assert!(matches[0].matched.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn borrowed_reference_resolution_order() {
    let owner = unique_owner();
    let env = Env::new(&names(&owner, &["home"])).await;
    // X is a rule in the bundled catalogue and in a repository the person added.
    let shared = markdown(
        "rule",
        "urn:ordin:default:rules:shared-style",
        "Shared style",
        "",
        "Write plainly.\n",
    );
    std::fs::create_dir_all(env.bundled.join("models/@default/rules")).unwrap();
    std::fs::write(
        env.bundled.join("models/@default/rules/shared-style.md"),
        &shared,
    )
    .unwrap();
    let mut person = BTreeMap::new();
    person.insert(
        "models/@default/rules/shared-style.md".to_owned(),
        shared.clone().into_bytes(),
    );
    commit_tree(&owner, "mine", &person);
    // The agent in P references X without holding it.
    let mut home = agents_repo(&["borrower"]);
    let (path, bytes) = agent_file("borrower", "Borrows a rule. It writes a summary.");
    home.insert(
        path,
        String::from_utf8(bytes)
            .unwrap()
            .replace(
                "defaultRules:",
                "defaultRules:\n  - urn:ordin:default:rules:shared-style",
            )
            .into_bytes(),
    );
    commit_tree(&owner, "home", &home);
    env.service
        .start(tokio_util::sync::CancellationToken::new())
        .await
        .unwrap();
    *env.github.permission.lock().unwrap() = RepoPermission::Read;
    let probe = env
        .service
        .probe_repository(ProbeRepositoryInput {
            web_address: format!("https://github.com/{owner}/mine"),
        })
        .await
        .unwrap();
    env.service
        .add_repository(AddRepositoryInput {
            probe_id: probe.probe_id,
            display_name: "Mine".into(),
        })
        .await
        .unwrap();
    env.service.fetch_now(None).await.unwrap();
    let home_id = repo_id(&env, "home").await;
    let scope = env
        .service
        .source_scope(RepositoryRef::Repository(home_id))
        .await
        .unwrap();
    let found = env
        .service
        .resolve(
            &scope,
            PartKind::Rule,
            "urn:ordin:default:rules:shared-style",
            None,
        )
        .await
        .unwrap();
    match found {
        Resolution::Found(part) => {
            assert_eq!(part.repository, RepositoryRef::Bundled);
            assert_eq!(part.repository_label, "bundled");
        }
        other => panic!("expected the bundled part, got {other:?}"),
    }
    let agents = env.service.search(LibraryQuery::default()).await.unwrap();
    assert!(
        agents
            .items
            .iter()
            .any(|i| i.part.part.identifier.ends_with("borrower")),
        "the borrowing agent is offered"
    );
}

use std::collections::BTreeMap;

async fn two_profile_repositories_sharing_y() -> (Env, ordin_types::AgentRepositoryId) {
    let owner = unique_owner();
    let env = Env::new(&names(&owner, &["home", "p1", "p2"])).await;
    commit_tree(&owner, "home", &agents_repo(&["own"]));
    let y = markdown("rule", "urn:ordin:acme:rules:y", "Y", "", "Y text.\n");
    commit_tree(
        &owner,
        "p1",
        &with(agents_repo(&["one"]), "models/@acme/rules/y.md", &y),
    );
    commit_tree(
        &owner,
        "p2",
        &with(agents_repo(&["two"]), "models/@acme/rules/y.md", &y),
    );
    env.service.sync_profile_repositories().await.unwrap();
    let home = repo_id(&env, "home").await;
    (env, home)
}

#[tokio::test(flavor = "multi_thread")]
async fn same_identifier_in_two_profile_repositories() {
    let (env, home) = two_profile_repositories_sharing_y().await;
    let scope = env
        .service
        .source_scope(RepositoryRef::Repository(home))
        .await
        .unwrap();
    match env
        .service
        .resolve(&scope, PartKind::Rule, "urn:ordin:acme:rules:y", None)
        .await
        .unwrap()
    {
        Resolution::Ambiguous { mut repositories } => {
            repositories.sort();
            assert_eq!(repositories.len(), 2);
            assert!(
                repositories[0].ends_with("/p1") && repositories[1].ends_with("/p2"),
                "{repositories:?}"
            );
        }
        other => panic!("expected Ambiguous, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_part_refused_at_ingest_is_not_found() {
    let owner = unique_owner();
    let env = Env::new(&names(&owner, &["agents"])).await;
    let skill = markdown(
        "skill",
        "urn:ordin:acme:skills:broken",
        "Broken",
        "  category: nosuchterm\n",
        "Text.\n",
    );
    commit_tree(
        &owner,
        "agents",
        &with(
            agents_repo(&["clean"]),
            "models/@acme/skills/broken/SKILL.md",
            &skill,
        ),
    );
    sync_one(&env).await;
    let id = repo_id(&env, "agents").await;
    let scope = env
        .service
        .source_scope(RepositoryRef::Repository(id))
        .await
        .unwrap();
    let found = env
        .service
        .resolve(
            &scope,
            PartKind::Skill,
            "urn:ordin:acme:skills:broken",
            None,
        )
        .await
        .unwrap();
    assert!(matches!(found, Resolution::NotFound), "{found:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_withdrawn_part_is_returned_flagged() {
    let owner = unique_owner();
    let env = Env::new(&names(&owner, &["agents"])).await;
    let (path, text) = rate_compare_skill();
    commit_tree(
        &owner,
        "agents",
        &with(
            with(agents_repo(&["first"]), &path, &text),
            "withdrawn.yaml",
            &withdrawn_yaml("1.2.0", DIGEST),
        ),
    );
    sync_one(&env).await;
    let id = repo_id(&env, "agents").await;
    let scope = env
        .service
        .source_scope(RepositoryRef::Repository(id))
        .await
        .unwrap();
    match env
        .service
        .resolve(
            &scope,
            PartKind::Skill,
            "urn:ordin:acme:skills:rate-compare",
            Some("1.2.0"),
        )
        .await
        .unwrap()
    {
        Resolution::Found(part) => assert!(part.part.withdrawn),
        other => panic!("expected Found, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn source_scope_of_a_person_added_repositorys_agent() {
    let owner = unique_owner();
    let env = Env::new(&names(&owner, &["p1", "p2"])).await;
    commit_tree(&owner, "p1", &agents_repo(&["one"]));
    commit_tree(&owner, "p2", &agents_repo(&["two"]));
    commit_tree(&owner, "q", &agents_repo(&["three"]));
    env.service.sync_profile_repositories().await.unwrap();
    let probe = env
        .service
        .probe_repository(ProbeRepositoryInput {
            web_address: format!("https://github.com/{owner}/q"),
        })
        .await
        .unwrap();
    let q = env
        .service
        .add_repository(AddRepositoryInput {
            probe_id: probe.probe_id,
            display_name: "Q".into(),
        })
        .await
        .unwrap()
        .id;
    let scope = env
        .service
        .source_scope(RepositoryRef::Repository(q))
        .await
        .unwrap();
    let p1 = repo_id(&env, "p1").await;
    let p2 = repo_id(&env, "p2").await;
    assert_eq!(
        scope,
        SourceScope {
            home: RepositoryRef::Repository(q),
            profile: vec![RepositoryRef::Repository(p1), RepositoryRef::Repository(p2)],
            include_bundled: true,
        }
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_check_failure_without_a_part_refuses_the_commit() {
    let owner = unique_owner();
    let env = Env::new(&names(&owner, &["agents"])).await;
    let commit_a = commit_tree(&owner, "agents", &agents_repo(&["first"]));
    sync_one(&env).await;
    // More than 500 findings: the Checker keeps 500 and adds `findings_truncated`, a Failure naming no part.
    let mut files = agents_repo(&["first"]);
    for n in 0..520 {
        files.insert(
            format!("models/@acme/rules/bad-{n}.md"),
            format!("---\napiVersion: ai.enterprise/v1\nkind: rule\nmetadata:\n  id: urn:ordin:acme:rules:bad-{n}\n  name: bad-{n}\n  version: 1.0.0\n  owner: \"\"\n---\nText.\n").into_bytes(),
        );
    }
    commit_tree(&owner, "agents", &files);
    let outcomes = env.service.fetch_now(None).await.unwrap();
    assert_eq!(
        outcomes[0].result,
        FetchResult::CommitRefused {
            reason_code: "CAT_CHECK_FAILED".into()
        }
    );
    let row = &env.service.list_repositories().await.unwrap()[0];
    assert_eq!(
        row.repository.last_fetched_commit.as_deref(),
        Some(commit_a.as_str())
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn company_drops_a_repository() {
    let owner = unique_owner();
    let env = Env::new(&names(&owner, &["agents"])).await;
    commit_tree(&owner, "agents", &agents_repo(&["first"]));
    sync_one(&env).await;
    let clone = env.service.list_repositories().await.unwrap()[0]
        .repository
        .clone_path
        .clone();
    assert!(std::path::Path::new(&clone).exists());
    let later = env.restart_with(&[]).await;
    let report = later.sync_profile_repositories().await.unwrap();
    assert!(report.results.is_empty());
    let state: String = sqlx::query_scalar("SELECT state FROM agent_repositories")
        .fetch_one(env.pool())
        .await
        .unwrap();
    assert_eq!(state, "dropped");
    assert!(!std::path::Path::new(&clone).exists());
    assert_eq!(env.count("SELECT COUNT(*) FROM parts").await, 0);
    assert!(later
        .search(LibraryQuery::default())
        .await
        .unwrap()
        .items
        .is_empty());
}

fn corrupt(path: &str) {
    std::fs::remove_dir_all(std::path::Path::new(path).join(".git")).expect("the clone's .git");
}

#[tokio::test(flavor = "multi_thread")]
async fn sync_profile_repositories_repairs_a_damaged_clone() {
    let owner = unique_owner();
    let env = Env::new(&[]).await;
    for name in ["p1", "p2", "p3"] {
        commit_tree(&owner, name, &agents_repo(&[name]));
    }
    let first = env
        .restart_with(&[
            Entry::ordin("p1", &owner, "p1"),
            Entry::ordin("p3", &owner, "p3"),
        ])
        .await;
    first.sync_profile_repositories().await.unwrap();
    let p3_clone = first
        .list_repositories()
        .await
        .unwrap()
        .into_iter()
        .find(|r| r.repository.repository.ends_with("/p3"))
        .unwrap()
        .repository
        .clone_path;
    corrupt(&p3_clone);
    let second = env.restart_with(&names(&owner, &["p1", "p2", "p3"])).await;
    let report = second.sync_profile_repositories().await.unwrap();
    let outcomes: Vec<_> = report.results.iter().map(|r| r.outcome.clone()).collect();
    assert_eq!(
        outcomes,
        [
            RepositorySyncOutcome::Fetched,
            RepositorySyncOutcome::Cloned,
            RepositorySyncOutcome::Repaired
        ]
    );
    assert!(
        std::path::Path::new(&p3_clone).join(".git").exists(),
        "the clone folder was cloned again"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn sync_profile_repositories_reports_one_unreachable_repository() {
    let owner = unique_owner();
    let env = Env::new(&[]).await;
    commit_tree(&owner, "p1", &agents_repo(&["one"]));
    env.restart_with(&names(&owner, &["p1"]))
        .await
        .sync_profile_repositories()
        .await
        .unwrap();
    // P2 has no repository behind its address.
    let second = env.restart_with(&names(&owner, &["p1", "p2"])).await;
    let report = second
        .sync_profile_repositories()
        .await
        .expect("the call itself succeeds");
    let outcomes: Vec<_> = report.results.iter().map(|r| r.outcome.clone()).collect();
    assert_eq!(
        outcomes,
        [
            RepositorySyncOutcome::Fetched,
            RepositorySyncOutcome::Failed {
                code: "CAT_UNREACHABLE".into()
            }
        ]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn requirement_list_over_50_is_refused_whole() {
    let env = Env::new(&[]).await;
    let error = env
        .service
        .match_requirements(MatchRequirementsInput {
            requirements: (0..51).map(|n| format!("requirement {n}")).collect(),
        })
        .await
        .unwrap_err();
    assert_eq!(error.code, "CAT_REQUIREMENTS_INVALID");
}

#[tokio::test(flavor = "multi_thread")]
async fn repository_named_by_profile_key() {
    let owner = unique_owner();
    let env = Env::new(&[Entry::ordin("acme-agents", &owner, "agents")]).await;
    commit_tree(&owner, "agents", &agents_repo(&["first"]));
    sync_one(&env).await;
    let found = env
        .service
        .repository_by_ref(&ordin_catalogue::AgentRepositoryRef::ProfileKey(
            "acme-agents".into(),
        ))
        .await
        .unwrap()
        .expect("the repository");
    assert_eq!(found.repository, format!("{owner}/agents"));
}

#[tokio::test(flavor = "multi_thread")]
async fn reviewers_not_readable_without_admin_rights_c_67_fallback() {
    let owner = unique_owner();
    let env = Env::new(&names(&owner, &["agents"])).await;
    commit_tree(&owner, "agents", &agents_repo(&["first"]));
    sync_one(&env).await;
    *env.github.branch_rules.lock().unwrap() = Err(GithubError::NotFound);
    let id = repo_id(&env, "agents").await;
    let info = env
        .service
        .reviewers_for(id, vec!["models/@acme/rules/tone.md".into()])
        .await
        .unwrap();
    assert!(!info.readable);
    assert!(info.reviewers.is_empty());
}

// ---- Slack for working together (ADR-102) ----

fn journey_file(slug: &str, body: &str) -> (String, String) {
    (
        format!("models/@acme/journeys/{slug}.yaml"),
        yaml(
            "journey",
            &format!("urn:ordin:acme:journeys:{slug}"),
            "Supplier onboarding",
            "",
            body,
        ),
    )
}

fn stage(id: &str, extra: &str) -> String {
    format!("  - id: {id}\n    name: {id}\n    role: Does the {id} work\n    handoverPlace:\n      type: git\n      target: https://github.com/acme/handover\n{extra}")
}

async fn with_journey(body: &str) -> (Env, ordin_types::AgentRepositoryId) {
    let owner = unique_owner();
    let env = Env::new(&names(&owner, &["agents"])).await;
    let (path, text) = journey_file("supplier-onboarding", body);
    commit_tree(
        &owner,
        "agents",
        &with(agents_repo(&["first"]), &path, &text),
    );
    sync_one(&env).await;
    let id = repo_id(&env, "agents").await;
    (env, id)
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stages_slack_channel_is_mapped() {
    let body = format!(
        "stages:\n{}{}",
        stage(
            "Design",
            "    slackChannel:\n      id: C04ABCDEF12\n      name: design-reviews\n"
        ),
        stage("Build", "")
    );
    let (env, id) = with_journey(&body).await;
    let lifecycles = env.service.lifecycles(Some(id)).await.unwrap();
    assert_eq!(lifecycles.len(), 1);
    let design = &lifecycles[0].stages[0];
    assert_eq!(design.name, "Design");
    let channel = design.slack_channel.as_ref().expect("a channel");
    assert_eq!(
        (channel.channel_id.as_str(), channel.name.as_str()),
        ("C04ABCDEF12", "design-reviews")
    );
    assert_eq!(
        design.next,
        StageNext::One {
            stage: "Build".into()
        }
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn lifecycles_without_a_slack_channel_keep_their_meaning() {
    let (env, id) =
        with_journey("seams:\n  - design-to-build\n  - build-to-run\ntrigger: merge\n").await;
    let lifecycles = env.service.lifecycles(Some(id)).await.unwrap();
    assert_eq!(lifecycles.len(), 1, "the lifecycle is offered");
    let names: Vec<_> = lifecycles[0]
        .stages
        .iter()
        .map(|s| s.name.as_str())
        .collect();
    assert_eq!(names, ["design", "build", "run"]);
    assert!(lifecycles[0]
        .stages
        .iter()
        .all(|s| s.slack_channel.is_none()));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_malformed_channel_is_dropped_not_the_lifecycle() {
    let body = format!(
        "stages:\n{}",
        stage(
            "Design",
            "    slackChannel:\n      id: design-reviews\n      name: design-reviews\n"
        )
    );
    let (env, id) = with_journey(&body).await;
    let lifecycles = env.service.lifecycles(Some(id)).await.unwrap();
    assert_eq!(lifecycles.len(), 1);
    assert!(lifecycles[0].stages[0].slack_channel.is_none());
    let report = env.service.health(id).await.unwrap();
    assert!(report
        .lines
        .iter()
        .any(|l| matches!(l, HealthLine::LifecycleSlackChannelIgnored { lifecycle, stage } if lifecycle == "Supplier onboarding" && stage == "Design")));
}

// ---- Jira issues and handovers (ADR-106) ----

fn jira_stage(target: &str) -> String {
    format!("stages:\n  - id: Analysis\n    name: Analysis\n    role: Analyses the request\n    handoverPlace:\n      type: jira\n      target: \"{target}\"\n")
}

#[tokio::test(flavor = "multi_thread")]
async fn happy_path_a_stage_names_a_jira_project_as_its_place() {
    let address = "https://acme.atlassian.net/jira/software/projects/ABC/boards/3";
    let (env, id) = with_journey(&jira_stage(address)).await;
    let lifecycles = env.service.lifecycles(Some(id)).await.unwrap();
    assert_eq!(lifecycles.len(), 1, "the lifecycle is offered");
    let place = env
        .service
        .lifecycle_place(lifecycles[0].stages[0].handover_place_id)
        .await
        .unwrap();
    assert_eq!(place.place_type, ordin_catalogue::HandoverPlaceType::Jira);
    assert_eq!(place.address, address);
}

#[tokio::test(flavor = "multi_thread")]
async fn error_path_a_jira_target_outside_jira_cloud() {
    let (env, id) = with_journey(&jira_stage("https://jira.acme.internal/projects/ABC")).await;
    assert!(env.service.lifecycles(Some(id)).await.unwrap().is_empty());
    let reason: Option<String> = sqlx::query_scalar("SELECT i.not_offered_reason FROM parts p JOIN part_index i ON i.part_id = p.id WHERE p.kind = 'Journey'")
        .fetch_one(env.pool())
        .await
        .unwrap();
    assert_eq!(reason.as_deref(), Some("LIFE_PLACE_INVALID"));
    let report = env.service.health(id).await.unwrap();
    assert!(report.lines.iter().any(
        |l| matches!(l, HealthLine::LifecyclePlaceInvalid { stage, .. } if stage == "Analysis")
    ));
}

#[tokio::test(flavor = "multi_thread")]
async fn edge_case_the_migration_keeps_every_place_id() {
    let dir = TempDir::new();
    let store = ordin_journal::Datastore::open(ordin_journal::DatastoreConfig::Sqlite {
        path: dir.0.join("journal.db"),
        backup_dir: dir.0.join("backups"),
    })
    .await
    .unwrap();
    let full = ordin_catalogue::migrations();
    let first_only = ordin_journal::ModuleMigrations {
        module: full.module,
        sqlite: &full.sqlite[..1],
        postgres: &[],
    };
    let mut registry = ordin_journal::MigrationRegistry::new();
    registry.register(ordin_journal::MIGRATIONS).unwrap();
    registry.register(first_only).unwrap();
    registry
        .apply(&store, ordin_journal::ApplyMode::ApplyPending)
        .await
        .unwrap();
    let pool = store.sqlite().unwrap();
    let (git, sharepoint) = (
        uuid::Uuid::now_v7().to_string(),
        uuid::Uuid::now_v7().to_string(),
    );
    for (id, kind, address) in [
        (&git, "git", "https://github.com/acme/handover"),
        (
            &sharepoint,
            "sharepoint",
            "https://acme.sharepoint.com/sites/x",
        ),
    ] {
        sqlx::query("INSERT INTO lifecycle_places (id, place_type, address) VALUES (?1, ?2, ?3)")
            .bind(id)
            .bind(kind)
            .bind(address)
            .execute(pool)
            .await
            .unwrap();
    }
    assert!(sqlx::query("INSERT INTO lifecycle_places (id, place_type, address) VALUES ('x', 'jira', 'https://a.atlassian.net/')").execute(pool).await.is_err());
    let mut registry = ordin_journal::MigrationRegistry::new();
    registry.register(ordin_journal::MIGRATIONS).unwrap();
    registry.register(ordin_catalogue::migrations()).unwrap();
    let report = registry
        .apply(&store, ordin_journal::ApplyMode::ApplyPending)
        .await
        .unwrap();
    assert!(report.applied.contains(&("catalogue".to_owned(), 2)));
    let ids: Vec<String> =
        sqlx::query_scalar("SELECT id FROM lifecycle_places ORDER BY place_type")
            .fetch_all(pool)
            .await
            .unwrap();
    assert_eq!(ids, [git, sharepoint]);
    sqlx::query("INSERT INTO lifecycle_places (id, place_type, address) VALUES ('j', 'jira', 'https://a.atlassian.net/')").execute(pool).await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn the_partsource_contract_test_passes() {
    let fixture = contract_fixture();
    let env = Env::new(&[]).await;
    // The bundled catalogue is a folder of the program; every part sits in a folder of its own.
    let bundled = &fixture.repositories[3].2;
    for (path, bytes) in tree_of_parts(bundled, &[]) {
        let target = env.bundled.join(&path);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(target, bytes).unwrap();
    }
    env.service
        .start(tokio_util::sync::CancellationToken::new())
        .await
        .expect("start");
    // Let the first reconciliation with the (empty) profile finish before the rows are made.
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let mut order = Vec::new();
    for (index, (reference, label, parts)) in fixture.repositories.iter().enumerate() {
        if index == 3 {
            continue;
        }
        let id = repository_row_id(reference);
        let source = if index == 4 { "person" } else { "profile" };
        env.insert_repository(id, label, source).await;
        let extra: Vec<(&str, String)> = fixture
            .withdrawn
            .iter()
            .filter(|w| w.agent_repository_id == id)
            .map(|w| {
                (
                    "withdrawn.yaml",
                    withdrawn_text("Skill", &w.part_identifier),
                )
            })
            .collect();
        let mut files = tree_of_parts(parts, &extra);
        if let Some((_, broken)) = fixture.refused.iter().find(|(r, _)| r == reference) {
            // The refused part fails its checks: its category is in no vocabulary.
            for (path, bytes) in files.iter_mut() {
                let text = String::from_utf8(bytes.clone()).unwrap();
                if text.contains(broken.as_str()) {
                    *bytes = text
                        .replacen("metadata:\n", "metadata:\n  category: nosuchterm\n", 1)
                        .into_bytes();
                    let _ = path;
                }
            }
        }
        let (owner, name) = label
            .strip_prefix("github.com/")
            .unwrap()
            .split_once('/')
            .unwrap();
        commit_tree(owner, name, &files);
        order.push(id);
    }
    for id in order {
        let outcomes = env.service.fetch_now(Some(id)).await.expect("fetch");
        assert!(
            matches!(outcomes[0].result, FetchResult::Ingested { .. }),
            "{:?}",
            outcomes[0].result
        );
    }
    let reasons: Vec<(String, Option<String>)> = sqlx::query_as("SELECT p.identifier || ' ' || p.version, i.not_offered_reason FROM parts p JOIN part_index i ON i.part_id = p.id WHERE i.offered = 0")
        .fetch_all(env.pool())
        .await
        .unwrap();
    println!("not offered: {reasons:?}");
    ordin_agent_model::testkit::assert_part_source_contract(&env.service, &fixture).await;
}
