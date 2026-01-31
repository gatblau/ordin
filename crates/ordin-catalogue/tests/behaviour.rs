//! Behaviour beyond the Error Table and the scenarios: the library reads, curation reads, draft
//! worktrees and sources, the bundled catalogue, repositories added and removed by the person.

mod support;

use std::collections::BTreeMap;

use ordin_agent_model::{PartKind, PartSource, RepositoryRef, Resolution};
use ordin_catalogue::{
    AddRepositoryInput, AgentRepositoryRef, Coverage, DraftWorktreeInput, FetchResult, HealthLine,
    LibraryQuery, MatchRequirementsInput, PartStateLabel, ProbeRepositoryInput,
};
use ordin_projects::{PullRequestView, ReviewDecision};
use ordin_types::domain::agents::{PullRequestState, VocabularyAxis, VocabularyTermState};
use support::*;

fn skill_text(slug: &str, version: &str, name: &str, description: &str, extra: &str) -> String {
    format!(
        "---\napiVersion: ai.enterprise/v1\nkind: skill\nmetadata:\n  id: urn:ordin:acme:skills:{slug}\n  name: {name}\n  version: {version}\n  owner: legal-ops\n  description: {description}\n{extra}---\nDo the {slug} work.\n"
    )
}

fn skill_path(slug: &str) -> String {
    format!("models/@acme/skills/{slug}/SKILL.md")
}

async fn one_repository(
    files: BTreeMap<String, Vec<u8>>,
) -> (Env, ordin_types::AgentRepositoryId, String) {
    let owner = unique_owner();
    let env = Env::new(&names(&owner, &["agents"])).await;
    commit_tree(&owner, "agents", &files);
    sync_one(&env).await;
    let id = repo_id(&env, "agents").await;
    (env, id, owner)
}

#[tokio::test(flavor = "multi_thread")]
async fn search_pages_by_a_cursor_and_facets_count_the_same_set() {
    let slugs: Vec<String> = (0..105).map(|n| format!("agent-{n:03}")).collect();
    let refs: Vec<&str> = slugs.iter().map(String::as_str).collect();
    let (env, id, _) = one_repository(agents_repo(&refs)).await;
    let first = env.service.search(LibraryQuery::default()).await.unwrap();
    assert_eq!(first.items.len(), 100);
    let cursor = first.next_cursor.clone().expect("a next page");
    let second = env
        .service
        .search(LibraryQuery {
            cursor: Some(cursor),
            ..LibraryQuery::default()
        })
        .await
        .unwrap();
    assert_eq!(second.items.len(), 5);
    assert!(second.next_cursor.is_none());
    let all: std::collections::BTreeSet<_> = first
        .items
        .iter()
        .chain(&second.items)
        .map(|i| i.part.part.id)
        .collect();
    assert_eq!(all.len(), 105, "no entry repeats across pages");
    let facets = env.service.facets(LibraryQuery::default()).await.unwrap();
    assert_eq!(facets.area, [("legal".to_owned(), 105)]);
    assert_eq!(facets.audience, [("lawyer".to_owned(), 105)]);
    assert_eq!(facets.publisher, [("acme".to_owned(), 105)]);
    assert_eq!(facets.repository, [(id, 105)]);
    let filtered = env
        .service
        .search(LibraryQuery {
            area: Some("none".into()),
            ..LibraryQuery::default()
        })
        .await
        .unwrap();
    assert!(filtered.items.is_empty());
    let by_text = env
        .service
        .search(LibraryQuery {
            text: Some("agent-007 reviews".into()),
            audience: Some("lawyer".into()),
            publisher: Some("acme".into()),
            repositories: vec![id],
            ..LibraryQuery::default()
        })
        .await
        .unwrap();
    assert_eq!(by_text.items.len(), 1);
    assert_eq!(by_text.items[0].part.part.name, "agent-007");
}

#[tokio::test(flavor = "multi_thread")]
async fn part_reads_used_by_made_of_and_history() {
    let mut files = agents_repo(&["helper"]);
    let (path, bytes) = agent_file("helper", "Helps. It writes a summary.");
    files.insert(
        path,
        String::from_utf8(bytes)
            .unwrap()
            .replace(
                "defaultRules:",
                "skills:\n  - urn:ordin:acme:skills:summarise\ndefaultRules:",
            )
            .into_bytes(),
    );
    files.insert(
        skill_path("summarise"),
        skill_text("summarise", "1.0.0", "Summarise", "Summarises a quote.", "").into_bytes(),
    );
    let (env, id, owner) = one_repository(files.clone()).await;
    // A second version replaces the first on the branch; the history keeps both.
    let mut next = files.clone();
    next.insert(
        skill_path("summarise"),
        skill_text(
            "summarise",
            "1.1.0",
            "Summarise",
            "Summarises a quote.",
            "  supersedes: urn:ordin:acme:skills:summarise\n",
        )
        .into_bytes(),
    );
    commit_tree(&owner, "agents", &next);
    env.service.fetch_now(None).await.unwrap();
    let skill = env
        .service
        .find_part(id, PartKind::Skill, "urn:ordin:acme:skills:summarise", None)
        .await
        .unwrap();
    assert_eq!(skill.part.version, "1.1.0");
    assert_eq!(skill.state_label, PartStateLabel::Current);
    let agent = env
        .service
        .find_part(
            id,
            PartKind::WorkspaceProfile,
            "urn:ordin:acme:workspaces:helper",
            None,
        )
        .await
        .unwrap();
    let doc = env.service.read_part(skill.part.id).await.unwrap();
    assert!(doc.file_text.contains("Do the summarise work."));
    assert_eq!(doc.view.part.id, skill.part.id);
    let used = env.service.used_by(skill.part.id).await.unwrap();
    assert_eq!(used.agents.len(), 1);
    assert_eq!(used.agents[0].part.id, agent.part.id);
    let made = env.service.made_of(agent.part.id).await.unwrap();
    let kinds: Vec<PartKind> = made.groups.iter().map(|(k, _)| *k).collect();
    assert_eq!(
        kinds,
        [PartKind::SystemRole, PartKind::Rule, PartKind::Skill]
    );
    let history = env.service.part_history(skill.part.id).await.unwrap();
    let versions: Vec<&str> = history
        .versions
        .iter()
        .map(|v| v.version.as_str())
        .collect();
    assert_eq!(versions, ["1.0.0", "1.1.0"]);
    let document = env.service.read_part(agent.part.id).await.unwrap();
    assert!(document.body.get("skills").is_some());
    let used_by_part = env.service.used_by(agent.part.id).await.unwrap();
    assert!(used_by_part.agents.is_empty() && used_by_part.parts.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn part_history_follows_the_supersedes_chain_both_ways() {
    let mut files = agents_repo(&["helper"]);
    files.insert(
        skill_path("old"),
        skill_text("old", "1.0.0", "Old", "Old skill.", "").into_bytes(),
    );
    files.insert(
        skill_path("new"),
        skill_text(
            "new",
            "1.0.0",
            "New",
            "New skill.",
            "  supersedes: urn:ordin:acme:skills:old\n",
        )
        .into_bytes(),
    );
    let (env, id, _) = one_repository(files).await;
    let old = env
        .service
        .find_part(id, PartKind::Skill, "urn:ordin:acme:skills:old", None)
        .await
        .unwrap();
    let new = env
        .service
        .find_part(id, PartKind::Skill, "urn:ordin:acme:skills:new", None)
        .await
        .unwrap();
    let from_new = env.service.part_history(new.part.id).await.unwrap();
    assert_eq!(from_new.replaces.len(), 1);
    assert_eq!(from_new.replaces[0].part.id, old.part.id);
    let from_old = env.service.part_history(old.part.id).await.unwrap();
    assert_eq!(from_old.replaced_by.len(), 1);
    assert_eq!(from_old.replaced_by[0].part.id, new.part.id);
}

#[tokio::test(flavor = "multi_thread")]
async fn invalid_supersedes_and_versions_withhold_the_part() {
    let mut files = agents_repo(&["helper"]);
    files.insert(
        skill_path("lost"),
        skill_text(
            "lost",
            "1.0.0",
            "Lost",
            "Lost skill.",
            "  supersedes: urn:ordin:acme:skills:nothing\n",
        )
        .into_bytes(),
    );
    files.insert(
        skill_path("odd"),
        skill_text("odd", "one", "Odd", "Odd skill.", "").into_bytes(),
    );
    let (env, _, _) = one_repository(files).await;
    let reasons: Vec<(String, Option<String>)> = sqlx::query_as("SELECT p.name, i.not_offered_reason FROM parts p JOIN part_index i ON i.part_id = p.id WHERE i.offered = 0 ORDER BY p.name")
        .fetch_all(env.pool())
        .await
        .unwrap();
    let reasons: BTreeMap<String, Option<String>> = reasons.into_iter().collect();
    assert_eq!(
        reasons.get("Lost").and_then(|r| r.as_deref()),
        Some("PART_SUPERSEDES_INVALID")
    );
    assert!(reasons.contains_key("Odd"), "{reasons:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn requirement_matching_by_words_covers_partly_or_not_at_all() {
    let mut files = agents_repo(&["helper"]);
    files.insert(
        skill_path("rates"),
        skill_text(
            "rates",
            "1.0.0",
            "Supplier day rates",
            "Compares supplier day rates across quotes.",
            "",
        )
        .into_bytes(),
    );
    let (env, _, _) = one_repository(files).await;
    let matches = env
        .service
        .match_requirements(MatchRequirementsInput {
            requirements: vec![
                "Compare supplier day rates across quotes".into(),
                "Draft the whole contract in French with a cover letter".into(),
                "Zebra quantum".into(),
            ],
        })
        .await
        .unwrap();
    assert_ne!(
        matches[0].coverage,
        Coverage::NotCovered,
        "{:?}",
        matches[0]
    );
    assert!(!matches[0].matched.is_empty());
    assert_eq!(matches[2].coverage, Coverage::NotCovered);
    assert!(matches[2].matched.is_empty());
    let similar = env
        .service
        .similar_parts(
            PartKind::Skill,
            "Compares supplier day rates across quotes",
            None,
        )
        .await
        .unwrap();
    assert_eq!(similar.len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn health_lists_unused_parts_look_alikes_withdrawn_uses_and_uncited_terms() {
    let mut files = agents_repo(&["helper"]);
    files.insert(
        skill_path("lonely"),
        skill_text(
            "lonely",
            "1.0.0",
            "Lonely",
            "Compares supplier day rates across quotes.",
            "",
        )
        .into_bytes(),
    );
    files.insert(
        skill_path("twin"),
        skill_text(
            "twin",
            "1.0.0",
            "Twin",
            "Compares supplier day rates across quotes.",
            "",
        )
        .into_bytes(),
    );
    // A deprecated rule the agent uses.
    let (rule_path, rule_text) = rule_file("retired", "  status: deprecated\n");
    files.insert(rule_path, rule_text.into_bytes());
    let (path, bytes) = agent_file("helper", "Helps. It writes a summary.");
    files.insert(
        path,
        String::from_utf8(bytes)
            .unwrap()
            .replace(
                "defaultRules:",
                "defaultRules:\n  - urn:ordin:acme:rules:retired",
            )
            .into_bytes(),
    );
    let (env, id, _) = one_repository(files).await;
    let report = loop {
        let report = env.service.health(id).await.unwrap();
        if report
            .lines
            .iter()
            .any(|l| matches!(l, HealthLine::LookAlike { .. }))
        {
            break report;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    };
    let kinds: Vec<&str> = report
        .lines
        .iter()
        .map(|l| match l {
            HealthLine::UnusedPart { .. } => "unused",
            HealthLine::LookAlike { .. } => "look_alike",
            HealthLine::UsedWhileDeprecated { .. } => "deprecated",
            HealthLine::UncitedTerm { .. } => "uncited",
            _ => "other",
        })
        .collect();
    for wanted in ["unused", "look_alike", "deprecated", "uncited"] {
        assert!(kinds.contains(&wanted), "{wanted} in {kinds:?}");
    }
    assert!(!report.commit.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn shared_identifiers_across_repositories_are_listed_in_the_health_report() {
    let owner = unique_owner();
    let env = Env::new(&names(&owner, &["one", "two"])).await;
    let shared = markdown(
        "rule",
        "urn:ordin:acme:rules:shared",
        "Shared",
        "",
        "Shared text.\n",
    );
    commit_tree(
        &owner,
        "one",
        &with(agents_repo(&["a"]), "models/@acme/rules/shared.md", &shared),
    );
    commit_tree(
        &owner,
        "two",
        &with(agents_repo(&["b"]), "models/@acme/rules/shared.md", &shared),
    );
    env.service.sync_profile_repositories().await.unwrap();
    // The report of `one` is recomputed when `one` changes, after `two` has been read.
    commit_tree(
        &owner,
        "one",
        &with(
            with(agents_repo(&["a"]), "models/@acme/rules/shared.md", &shared),
            "models/@acme/rules/extra.md",
            &markdown(
                "rule",
                "urn:ordin:acme:rules:extra",
                "Extra",
                "",
                "Extra.\n",
            ),
        ),
    );
    let one = repo_id(&env, "one").await;
    env.service.fetch_now(Some(one)).await.unwrap();
    let mut lines = Vec::new();
    for _ in 0..100 {
        lines = env.service.health(one).await.unwrap().lines;
        if lines
            .iter()
            .any(|l| matches!(l, HealthLine::SharedIdentifier { .. }))
        {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(lines.iter().any(|l| matches!(l, HealthLine::SharedIdentifier { identifier, .. } if identifier == "urn:ordin:acme:rules:shared")), "{lines:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn layout_vocabulary_and_withdrawn_reads() {
    let mut files = agents_repo(&["helper"]);
    files.insert(
        "models/@acme/rules/README.md".into(),
        b"# Rules\n\nHouse rules.\n".to_vec(),
    );
    let (env, id, owner) = one_repository(files.clone()).await;
    let layout = env.service.layout(id).await.unwrap();
    let rules = layout
        .kinds
        .iter()
        .find(|k| k.kind == PartKind::Rule)
        .expect("rules");
    assert_eq!(rules.directory, "models/@acme/rules");
    assert_eq!(rules.count, 1);
    assert!(rules
        .readme
        .as_deref()
        .unwrap_or("")
        .contains("House rules."));
    assert!(layout
        .kinds
        .iter()
        .any(|k| k.kind == PartKind::Vocabulary && k.readme.is_none()));
    let terms = env.service.vocabulary(id).await.unwrap();
    assert!(terms.iter().any(|t| t.axis == VocabularyAxis::Category
        && t.code == "legal"
        && t.state == VocabularyTermState::Active));
    // The category vocabulary drops its term: it is retired, not deleted.
    let mut next = files;
    next.insert(
        "models/@acme/vocabulary/categories.yaml".into(),
        yaml(
            "vocabulary",
            "urn:ordin:acme:vocabulary:categories",
            "categories",
            "",
            "axis: category\nterms:\n  - code: other\n    label: Other\n",
        )
        .into_bytes(),
    );
    commit_tree(&owner, "agents", &next);
    env.service.fetch_now(None).await.unwrap();
    let terms = env.service.vocabulary(id).await.unwrap();
    let legal = terms.iter().find(|t| t.code == "legal").expect("legal");
    assert_eq!(legal.state, VocabularyTermState::Retired);
    assert!(env.service.withdrawn_list(id).await.unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn reviewers_come_from_codeowners_and_pull_requests_report_signing() {
    let mut files = agents_repo(&["helper"]);
    files.insert(
        ".github/CODEOWNERS".into(),
        b"# owners\n* @acme/everyone\n/models/@acme/rules/ @alice @bob\n*.yaml @carol\n".to_vec(),
    );
    let (env, id, _) = one_repository(files).await;
    let info = env
        .service
        .reviewers_for(
            id,
            vec![
                "models/@acme/rules/tone.md".into(),
                "models/@acme/vocabulary/categories.yaml".into(),
            ],
        )
        .await
        .unwrap();
    assert!(info.readable);
    assert_eq!(info.review_required, Some(true));
    assert_eq!(info.reviewers, ["@alice", "@bob", "@carol"]);
    let none = env
        .service
        .reviewers_for(id, vec!["README.md".into()])
        .await
        .unwrap();
    assert_eq!(none.reviewers, ["@acme/everyone"]);
    // Pull request status; signing is read only where the repository requires it.
    *env.github.pull_request.lock().unwrap() = Some(PullRequestView {
        number: 7,
        state: PullRequestState::Merged,
        review: ReviewDecision::Approved,
        change_requests: Vec::new(),
        merge_commit: Some("a".repeat(40)),
        head: "draft".into(),
        base: "main".into(),
        url: "https://github.com/acme/agents/pull/7".into(),
    });
    let status = env.service.pull_request_status(id, 7).await.unwrap();
    assert_eq!(status.state, PullRequestState::Merged);
    assert_eq!(status.merged_signed, None);
    assert!(env.service.pull_request_status(id, 8).await.is_ok());
}

#[tokio::test(flavor = "multi_thread")]
async fn signed_merges_are_checked_on_a_merged_pull_request() {
    let owner = unique_owner();
    let mut entry = Entry::ordin("agents", &owner, "agents");
    entry.signed = true;
    let env = Env::new(&[entry]).await;
    commit_tree(&owner, "agents", &agents_repo(&["first"]));
    env.service.sync_profile_repositories().await.unwrap();
    let id = repo_id(&env, "agents").await;
    *env.github.pull_request.lock().unwrap() = Some(PullRequestView {
        number: 7,
        state: PullRequestState::Merged,
        review: ReviewDecision::Approved,
        change_requests: Vec::new(),
        merge_commit: Some("b".repeat(40)),
        head: "draft".into(),
        base: "main".into(),
        url: String::new(),
    });
    assert_eq!(
        env.service
            .pull_request_status(id, 7)
            .await
            .unwrap()
            .merged_signed,
        Some(true)
    );
    *env.github.verified.lock().unwrap() = false;
    assert_eq!(
        env.service
            .pull_request_status(id, 7)
            .await
            .unwrap()
            .merged_signed,
        Some(false)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_draft_worktree_shadows_the_head_in_the_draft_source() {
    let mut files = agents_repo(&["helper"]);
    files.insert(
        skill_path("summarise"),
        skill_text("summarise", "1.0.0", "Summarise", "Summarises.", "").into_bytes(),
    );
    let (env, id, _) = one_repository(files).await;
    let opened = env
        .service
        .open_draft_worktree(DraftWorktreeInput {
            agent_repository_id: id,
            branch: "draft/new-skill".into(),
        })
        .await
        .unwrap();
    assert_eq!(opened.base_commit.len(), 40);
    // The author adds a skill, changes another and deletes nothing.
    std::fs::create_dir_all(opened.path.join("models/@acme/skills/drafted")).unwrap();
    std::fs::write(
        opened.path.join(skill_path("drafted")),
        skill_text(
            "drafted",
            "0.1.0",
            "Drafted",
            "A new skill.",
            "  status: draft\n",
        ),
    )
    .unwrap();
    std::fs::write(
        opened.path.join(skill_path("summarise")),
        skill_text("summarise", "1.1.0", "Summarise", "Summarises better.", ""),
    )
    .unwrap();
    let source = env
        .service
        .draft_source(id, opened.path.clone())
        .await
        .unwrap();
    let scope = env
        .service
        .source_scope(RepositoryRef::Repository(id))
        .await
        .unwrap();
    // A worktree part is visible whatever its status, and shadows the indexed part.
    let Resolution::Found(drafted) = source
        .resolve(
            &scope,
            PartKind::Skill,
            "urn:ordin:acme:skills:drafted",
            None,
        )
        .await
        .unwrap()
    else {
        panic!("the draft skill")
    };
    assert_eq!(drafted.part.commit, "0".repeat(40));
    assert_eq!(source.files(&drafted).await.unwrap().0.len(), 1);
    let Resolution::Found(summarise) = source
        .resolve(
            &scope,
            PartKind::Skill,
            "urn:ordin:acme:skills:summarise",
            None,
        )
        .await
        .unwrap()
    else {
        panic!("the skill")
    };
    assert_eq!(summarise.part.version, "1.1.0");
    assert!(matches!(
        source
            .resolve_any_kind(&scope, "urn:ordin:acme:skills:drafted")
            .await
            .unwrap(),
        Resolution::Found(_)
    ));
    assert!(matches!(
        source
            .resolve(
                &scope,
                PartKind::Skill,
                "urn:ordin:acme:skills:nothing",
                None
            )
            .await
            .unwrap(),
        Resolution::NotFound
    ));
    // The indexed catalogue is unchanged.
    assert!(matches!(
        env.service
            .resolve(
                &scope,
                PartKind::Skill,
                "urn:ordin:acme:skills:drafted",
                None
            )
            .await
            .unwrap(),
        Resolution::NotFound
    ));
    let parts = source
        .repository_parts(&RepositoryRef::Repository(id))
        .await
        .unwrap();
    assert!(parts.iter().any(|p| p.part.identifier.ends_with("drafted")));
    let vocabulary = source.vocabulary(&scope).await.unwrap();
    assert!(vocabulary.has(VocabularyAxis::Category, "legal"));
    assert!(source.withdrawn(&scope).await.unwrap().is_empty());
    assert!(source.ingredients(&scope).await.unwrap().is_empty());
    assert!(source
        .citing_parts(
            &RepositoryRef::Repository(id),
            VocabularyAxis::Category,
            "legal"
        )
        .await
        .unwrap()
        .iter()
        .any(|p| p.part.identifier.ends_with("helper")));
    // The draft agent's closure reaches the draft skill when the agent names it.
    std::fs::write(
        opened
            .path
            .join("models/@acme/workspaces/helper/workspace-profile.yaml"),
        String::from_utf8(agent_file("helper", "Helps. It writes a summary.").1)
            .unwrap()
            .replace(
                "defaultRules:",
                "skills:\n  - urn:ordin:acme:skills:drafted\ndefaultRules:",
            ),
    )
    .unwrap();
    let source = env
        .service
        .draft_source(id, opened.path.clone())
        .await
        .unwrap();
    let Resolution::Found(drafted) = source
        .resolve(
            &scope,
            PartKind::Skill,
            "urn:ordin:acme:skills:drafted",
            None,
        )
        .await
        .unwrap()
    else {
        panic!("the draft skill")
    };
    let users = source.used_by(&drafted).await.unwrap();
    assert_eq!(users.len(), 1);
    assert!(users[0].part.identifier.ends_with("helper"));
    env.service
        .close_draft_worktree("draft/new-skill", id)
        .await
        .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn the_bundled_catalogue_is_indexed_again_only_when_its_version_or_files_change() {
    let env = Env::new(&[]).await;
    std::fs::create_dir_all(env.bundled.join("models/@default/rules")).unwrap();
    std::fs::write(
        env.bundled.join("models/@default/rules/plain.md"),
        markdown(
            "rule",
            "urn:ordin:default:rules:plain",
            "Plain",
            "",
            "Write plainly.\n",
        ),
    )
    .unwrap();
    let stop = tokio_util::sync::CancellationToken::new();
    env.service.start(stop.clone()).await.unwrap();
    assert_eq!(env.count("SELECT COUNT(*) FROM parts WHERE agent_repository_id = '01900000-0000-7000-8000-000000000001'").await, 1);
    let scope = ordin_agent_model::SourceScope {
        home: RepositoryRef::Bundled,
        profile: Vec::new(),
        include_bundled: true,
    };
    let Resolution::Found(plain) = env
        .service
        .resolve(
            &scope,
            PartKind::Rule,
            "urn:ordin:default:rules:plain",
            None,
        )
        .await
        .unwrap()
    else {
        panic!("bundled rule")
    };
    assert_eq!(plain.part.commit, "ordin-0.1.0-test");
    assert_eq!(plain.repository_display_name, "ordin");
    // CAT-BND-1: the same version with the same files is not read again. A name changed in the
    // table by hand stays, which it would not after an ingest.
    // spec: ordin-desktop/components/CatalogueService#CAT-BND-1
    sqlx::query("UPDATE parts SET name = 'Not read again' WHERE identifier = 'urn:ordin:default:rules:plain'")
        .execute(env.pool())
        .await
        .unwrap();
    let again = env.restart_with(&[]).await;
    again.start(stop.clone()).await.unwrap();
    assert_eq!(env.count("SELECT COUNT(*) FROM parts").await, 1);
    assert_eq!(
        env.count("SELECT COUNT(*) FROM parts WHERE name = 'Not read again'")
            .await,
        1
    );
    // The same version with other bundled agents is read again.
    std::fs::write(
        env.bundled.join("models/@default/rules/more.md"),
        markdown(
            "rule",
            "urn:ordin:default:rules:more",
            "More",
            "",
            "More.\n",
        ),
    )
    .unwrap();
    let rebuilt = env.restart_with(&[]).await;
    rebuilt.start(stop.clone()).await.unwrap();
    assert_eq!(env.count("SELECT COUNT(*) FROM parts").await, 2);
    assert_eq!(
        env.count("SELECT COUNT(*) FROM parts WHERE name = 'Not read again'")
            .await,
        0
    );
    assert_eq!(
        env.count("SELECT COUNT(*) FROM agent_repositories WHERE bundled_digest IS NOT NULL")
            .await,
        1
    );
    assert_eq!(
        env.service
            .repository_ref(ordin_agent_model::BUNDLED_REPOSITORY_ID)
            .await
            .unwrap(),
        AgentRepositoryRef::ProfileKey("ordin".into())
    );
    let found = env
        .service
        .repository_by_ref(&AgentRepositoryRef::ProfileKey("ordin".into()))
        .await
        .unwrap()
        .expect("the bundled row");
    assert_eq!(found.host, "bundled");
    assert!(env
        .service
        .list_repositories()
        .await
        .unwrap()
        .iter()
        .any(|r| r.repository.id == ordin_agent_model::BUNDLED_REPOSITORY_ID));
    stop.cancel();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_person_removes_their_own_repository_and_cannot_remove_it_twice() {
    let env = Env::new(&[]).await;
    let owner = unique_owner();
    commit_tree(&owner, "mine", &agents_repo(&["first"]));
    let probe = env
        .service
        .probe_repository(ProbeRepositoryInput {
            web_address: format!("https://github.com/{owner}/mine"),
        })
        .await
        .unwrap();
    let added = env
        .service
        .add_repository(AddRepositoryInput {
            probe_id: probe.probe_id,
            display_name: "Mine".into(),
        })
        .await
        .unwrap();
    assert_eq!(
        env.service.repository_ref(added.id).await.unwrap(),
        AgentRepositoryRef::Address(format!("https://github.com/{owner}/mine"))
    );
    let by_address = env
        .service
        .repository_by_ref(&AgentRepositoryRef::Address(format!(
            "https://github.com/{owner}/mine.git"
        )))
        .await
        .unwrap();
    assert_eq!(by_address.map(|r| r.id), Some(added.id));
    let mut notices = env.service.subscribe();
    env.service.remove_repository(added.id).await.unwrap();
    assert!(matches!(
        notices.try_recv(),
        Ok(ordin_catalogue::CatalogueNotice::RepositoryDropped { .. })
    ));
    assert!(!std::path::Path::new(&added.clone_path).exists());
    assert!(env
        .service
        .search(LibraryQuery::default())
        .await
        .unwrap()
        .items
        .is_empty());
    assert!(env
        .service
        .repository_by_ref(&AgentRepositoryRef::Address(format!(
            "https://github.com/{owner}/mine"
        )))
        .await
        .unwrap()
        .is_none());
    // The address can be added again.
    let probe = env
        .service
        .probe_repository(ProbeRepositoryInput {
            web_address: format!("https://github.com/{owner}/mine"),
        })
        .await
        .unwrap();
    assert_eq!(
        env.service
            .add_repository(AddRepositoryInput {
                probe_id: probe.probe_id,
                display_name: "Mine again".into()
            })
            .await
            .unwrap()
            .display_name,
        "Mine again"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_fetch_with_nothing_new_is_unchanged_and_a_new_commit_notifies_subscribers() {
    let owner = unique_owner();
    let env = Env::new(&names(&owner, &["agents"])).await;
    commit_tree(&owner, "agents", &agents_repo(&["first"]));
    let mut notices = env.service.subscribe();
    env.service.sync_profile_repositories().await.unwrap();
    assert!(matches!(
        notices.try_recv(),
        Ok(ordin_catalogue::CatalogueNotice::RepositoryUpdated { .. })
    ));
    let outcomes = env.service.fetch_now(None).await.unwrap();
    assert_eq!(outcomes[0].result, FetchResult::Unchanged);
    assert!(outcomes[0].commit.is_some());
    let summary = &env.service.list_repositories().await.unwrap()[0];
    assert_eq!(summary.agent_count, 1);
    assert!(summary.set_by_company);
}

fn journey_body(stages: &str) -> String {
    format!("stages:\n{stages}")
}

fn plain_stage(id: &str, role: &str, extra: &str) -> String {
    format!("  - id: {id}\n    name: {id}\n    role: \"{role}\"\n    handoverPlace:\n      type: git\n      target: https://github.com/acme/handover\n{extra}")
}

#[tokio::test(flavor = "multi_thread")]
async fn lifecycles_breaking_a_rule_are_not_offered_and_branches_are_read() {
    let cases: Vec<(&str, String)> = vec![
        ("LIFE_STAGES_INVALID", journey_body(&format!("{}{}", plain_stage("A", "r", ""), plain_stage("A", "r", "")))),
        ("LIFE_ROLE_INVALID", journey_body(&plain_stage("A", "", ""))),
        ("LIFE_DAYS_OUT_OF_RANGE", journey_body(&plain_stage("A", "r", "    answerWithinDays: 400\n"))),
        ("LIFE_STAGE_BACKWARD", journey_body(&format!("{}{}", plain_stage("A", "r", "    next:\n      - stage: B\n"), plain_stage("B", "r", "    next:\n      - stage: A\n")))),
        ("LIFE_SEND_BACK_NOT_EARLIER", journey_body(&format!("{}{}", plain_stage("A", "r", "    sendsBackTo:\n      - stage: B\n        when: never\n        maxRounds: 2\n"), plain_stage("B", "r", "")))),
        ("LIFE_SEND_BACK_ROUNDS_OUT_OF_RANGE", journey_body(&format!("{}{}", plain_stage("A", "r", ""), plain_stage("B", "r", "    sendsBackTo:\n      - stage: A\n        when: late\n        maxRounds: 20\n")))),
        ("LIFE_STAGE_UNREACHABLE", journey_body(&format!("{}{}", plain_stage("A", "r", "    next: []\n"), plain_stage("B", "r", "")))),
        ("LIFE_CHOICES_INVALID", journey_body(&format!("{}{}{}{}{}{}", plain_stage("A", "r", "    next:\n      - stage: B\n        when: x\n      - stage: C\n        when: y\n      - stage: D\n        when: z\n      - stage: E\n        when: w\n      - stage: F\n        when: v\n"), plain_stage("B", "r", ""), plain_stage("C", "r", ""), plain_stage("D", "r", ""), plain_stage("E", "r", ""), plain_stage("F", "r", "")))),
        ("JOURNEY_SEAMS_LOOP", "seams:\n  - a-to-b\n  - b-to-a\n".to_owned()),
        ("LIFE_STAGES_INVALID", "seams: []\nstages: []\n".to_owned()),
    ];
    for (code, body) in cases {
        let owner = unique_owner();
        let env = Env::new(&names(&owner, &["agents"])).await;
        let path = "models/@acme/journeys/flow.yaml";
        let text = yaml("journey", "urn:ordin:acme:journeys:flow", "Flow", "", &body);
        commit_tree(
            &owner,
            "agents",
            &with(agents_repo(&["first"]), path, &text),
        );
        sync_one(&env).await;
        let id = repo_id(&env, "agents").await;
        let reason: Option<String> = sqlx::query_scalar("SELECT i.not_offered_reason FROM parts p JOIN part_index i ON i.part_id = p.id WHERE p.kind = 'Journey'")
            .fetch_one(env.pool())
            .await
            .unwrap();
        assert!(
            env.service.lifecycles(Some(id)).await.unwrap().is_empty(),
            "{code}"
        );
        // The Checker may withhold the part first for the same rule in its own words; either way it is not offered.
        assert!(reason.is_some(), "{code}");
    }
    // Choices, side-by-side stages and send-backs that follow the rules are read.
    let owner = unique_owner();
    let env = Env::new(&names(&owner, &["agents"])).await;
    let body = journey_body(&format!(
        "{}{}{}{}",
        plain_stage("A", "Starts", "    answerWithinDays: 3\n    next:\n      - stage: B\n        when: easy\n      - stage: C\n        when: hard\n"),
        plain_stage("B", "Easy path", "    sendsBackTo:\n      - stage: A\n        when: unclear\n        maxRounds: 2\n    next:\n      - stage: D\n"),
        plain_stage("C", "Hard path", "    next:\n      - stage: D\n"),
        plain_stage("D", "Ends", ""),
    ));
    let text = yaml("journey", "urn:ordin:acme:journeys:flow", "Flow", "", &body);
    commit_tree(
        &owner,
        "agents",
        &with(
            agents_repo(&["first"]),
            "models/@acme/journeys/flow.yaml",
            &text,
        ),
    );
    sync_one(&env).await;
    let lifecycles = env.service.lifecycles(None).await.unwrap();
    assert_eq!(lifecycles.len(), 1, "{:?}", sqlx::query_as::<_, (String, Option<String>)>("SELECT p.name, i.not_offered_reason FROM parts p JOIN part_index i ON i.part_id = p.id WHERE i.offered = 0").fetch_all(env.pool()).await.unwrap());
    let by_id = env.service.lifecycle(lifecycles[0].id).await.unwrap();
    assert_eq!(by_id.stages.len(), 4);
    assert_eq!(by_id.stages[0].days_to_answer, 3);
    assert!(
        matches!(by_id.stages[0].next, ordin_types::domain::agents::StageNext::OneOf { ref choices } if choices.len() == 2)
    );
    assert_eq!(by_id.stages[1].send_backs[0].max_rounds, 2);
    assert_eq!(
        by_id.stages[0].handover_place_id, by_id.stages[3].handover_place_id,
        "one place id per distinct place"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn citations_resolve_to_the_vocabulary_of_another_profile_repository() {
    let owner = unique_owner();
    let env = Env::new(&names(&owner, &["vocab", "user"])).await;
    commit_tree(&owner, "vocab", &agents_repo(&["one"]));
    // The second repository defines no vocabulary of its own and cites a term of the first.
    let mut user = BTreeMap::new();
    let (path, text) = rule_file("cites", "  category: legal\n");
    user.insert(path, text.into_bytes());
    commit_tree(&owner, "user", &user);
    env.service.sync_profile_repositories().await.unwrap();
    let vocab = repo_id(&env, "vocab").await;
    let citing = env
        .service
        .citing_parts(
            &RepositoryRef::Repository(vocab),
            VocabularyAxis::Category,
            "legal",
        )
        .await
        .unwrap();
    let names: Vec<&str> = citing.iter().map(|p| p.part.name.as_str()).collect();
    assert!(
        names.contains(&"cites") && names.contains(&"one"),
        "{names:?}"
    );
    assert!(env
        .service
        .citing_parts(
            &RepositoryRef::Repository(vocab),
            VocabularyAxis::Category,
            "unknown"
        )
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_withdrawn_part_is_excluded_from_the_library_and_labelled() {
    let owner = unique_owner();
    let env = Env::new(&names(&owner, &["agents"])).await;
    let mut files = agents_repo(&["first"]);
    files.insert(
        skill_path("old"),
        skill_text("old", "1.0.0", "Old", "Old skill.", "").into_bytes(),
    );
    files.insert(
        "withdrawn.yaml".into(),
        withdrawn_text("Skill", "urn:ordin:acme:skills:old").into_bytes(),
    );
    commit_tree(&owner, "agents", &files);
    sync_one(&env).await;
    let id = repo_id(&env, "agents").await;
    let skills = env
        .service
        .search(LibraryQuery {
            kinds: vec![PartKind::Skill],
            ..LibraryQuery::default()
        })
        .await
        .unwrap();
    assert!(skills.items.is_empty(), "withdrawn parts leave the library");
    let view = env
        .service
        .find_part(id, PartKind::Skill, "urn:ordin:acme:skills:old", None)
        .await
        .unwrap();
    assert_eq!(view.state_label, PartStateLabel::Withdrawn);
    let scope = env
        .service
        .source_scope(RepositoryRef::Repository(id))
        .await
        .unwrap();
    assert_eq!(env.service.withdrawn(&scope).await.unwrap().len(), 1);
    let unused: i64 = env
        .count("SELECT COUNT(*) FROM withdrawn_list_entries")
        .await;
    assert_eq!(unused, 1);
    // The append-only trigger refuses a change of the reason.
    let refused = sqlx::query("UPDATE withdrawn_list_entries SET reason = 'changed'")
        .execute(env.pool())
        .await;
    assert!(refused.unwrap_err().to_string().contains("WDL_APPEND_ONLY"));
}

#[tokio::test(flavor = "multi_thread")]
async fn ingredients_overlays_and_packs_are_listed_in_scope_order() {
    let owner = unique_owner();
    let env = Env::new(&names(&owner, &["agents"])).await;
    let mut files = agents_repo(&["first"]);
    files.insert(
        "models/@acme/practice-packs/stack/go.md".into(),
        markdown("practice-pack", "urn:ordin:acme:practice-packs:stack:go", "Go", "  category: legal\n  class: stack\n  audience:\n    roles: [lawyer]\n  description: Go stack. It sets Go rules.\n", "Use Go.\n").into_bytes(),
    );
    commit_tree(&owner, "agents", &files);
    sync_one(&env).await;
    let id = repo_id(&env, "agents").await;
    let scope = env
        .service
        .source_scope(RepositoryRef::Repository(id))
        .await
        .unwrap();
    let ingredients = env.service.ingredients(&scope).await.unwrap();
    assert_eq!(
        ingredients.iter().map(|p| p.part.kind).collect::<Vec<_>>(),
        [PartKind::PracticePack],
        "{ingredients:?}"
    );
    let vocabulary = PartSource::vocabulary(&env.service, &scope).await;
    assert!(vocabulary.is_ok());
    let any = env
        .service
        .resolve_any_kind(&scope, "urn:ordin:acme:rules:tone")
        .await
        .unwrap();
    assert!(matches!(any, Resolution::Found(_)));
}

/// Writes the base parts and a WorkspaceProfile with the identifier `urn:ordin:default:workspaces:<slug>`
/// into the bundled catalogue folder.
fn bundled_profile(env: &Env, slug: &str) {
    let path = env.bundled.join(format!(
        "models/@default/workspaces/{slug}/workspace-profile.yaml"
    ));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let text = yaml(
        "workspace-profile",
        &format!("urn:ordin:default:workspaces:{slug}"),
        slug,
        &format!("  description: {slug} reviews contracts. It writes a summary.\n{DESCRIPTIVE}"),
        &format!("systemRole: {ROLE_ID}\ndefaultRules:\n  - {TONE_ID}\n"),
    );
    std::fs::write(path, text).unwrap();
}

async fn bundled_with_coordinator() -> Env {
    let env = Env::new(&[]).await;
    for (path, bytes) in base_files() {
        let target = env.bundled.join(&path);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(target, bytes).unwrap();
    }
    bundled_profile(&env, "coordinator");
    bundled_profile(&env, "document-agent");
    env.service
        .start(tokio_util::sync::CancellationToken::new())
        .await
        .unwrap();
    env
}

/// spec: ordin-desktop/components/CatalogueService#CAT-LIB-1
#[tokio::test(flavor = "multi_thread")]
async fn cat_lib_1_a_search_finds_no_coordinator_and_the_facets_do_not_count_it() {
    let env = bundled_with_coordinator().await;
    let mut names = Vec::new();
    for _ in 0..50 {
        let page = env.service.search(LibraryQuery::default()).await.unwrap();
        names = page
            .items
            .iter()
            .map(|i| i.part.part.name.clone())
            .collect();
        if !names.is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert_eq!(names, ["document-agent"], "the coordinator is left out");
    let facets = env.service.facets(LibraryQuery::default()).await.unwrap();
    assert_eq!(
        facets.repository,
        [(ordin_catalogue::BUNDLED_REPOSITORY_ID, 1)],
        "the bundled repository's count excludes it"
    );
}

/// spec: ordin-desktop/components/CatalogueService#CAT-LIB-1
#[tokio::test(flavor = "multi_thread")]
async fn cat_lib_1_the_coordinator_part_can_still_be_read() {
    let env = bundled_with_coordinator().await;
    // The library lists the document agent once the bundled catalogue is read; the coordinator is read by its part.
    let mut listed = false;
    for _ in 0..50 {
        let page = env.service.search(LibraryQuery::default()).await.unwrap();
        if !page.items.is_empty() {
            listed = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(listed, "the bundled catalogue was read");
    let found = env
        .service
        .resolve(
            &env.service
                .source_scope(ordin_agent_model::RepositoryRef::Bundled)
                .await
                .unwrap(),
            PartKind::WorkspaceProfile,
            "urn:ordin:default:workspaces:coordinator",
            None,
        )
        .await
        .unwrap();
    assert!(matches!(found, Resolution::Found(_)), "got {found:?}");
}
