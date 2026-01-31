//! X1: one test per row of CatalogueService.md *Error Table*.

mod support;

use ordin_agent_model::{PartKind, PartSource, PartSourceError, RepositoryRef, SourceScope};
use ordin_catalogue::{
    AddRepositoryInput, AgentRepositoryRef, DraftWorktreeInput, FetchResult, LibraryQuery,
    MatchRequirementsInput, ProbeRepositoryInput, RenameRepositoryInput, RepositorySyncOutcome,
};
use ordin_projects::{GithubError, RepoPermission};
use ordin_types::paging::Cursor;
use ordin_types::{AgentRepositoryId, CodedError, RepairAction};
use support::*;

async fn probe(
    env: &Env,
    address: &str,
) -> Result<ordin_catalogue::RepositoryProbe, ordin_catalogue::CatalogueError> {
    env.service
        .probe_repository(ProbeRepositoryInput {
            web_address: address.to_owned(),
        })
        .await
}

#[tokio::test(flavor = "multi_thread")]
async fn web_address_not_a_repository_url() {
    let env = Env::new(&[]).await;
    for address in [
        "https://github.com/onlyowner",
        "not an address",
        "http://github.com/a/b",
        "https://github.com/a/b/c",
        "https://github.com/a b/c",
    ] {
        let error = probe(&env, address).await.unwrap_err();
        assert_eq!(error.code, "AREPO_NAME_INVALID", "{address}");
        assert_eq!(error.action, None);
        assert!(!error.retryable);
    }
    let long = format!("https://github.com/a/{}", "b".repeat(2100));
    assert_eq!(
        probe(&env, &long).await.unwrap_err().code,
        "AREPO_NAME_INVALID"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn host_is_not_github_or_a_profile_host() {
    let env = Env::new(&[]).await;
    let error = probe(&env, "https://gitlab.com/acme/agents")
        .await
        .unwrap_err();
    assert_eq!(error.code, "AREPO_HOST_UNSUPPORTED");
    assert_eq!(error.action, None);
}

#[tokio::test(flavor = "multi_thread")]
async fn repository_already_read() {
    let owner = unique_owner();
    let env = Env::new(&names(&owner, &["agents"])).await;
    commit_tree(&owner, "agents", &agents_repo(&["first"]));
    sync_one(&env).await;
    let error = probe(&env, &format!("https://github.com/{owner}/agents"))
        .await
        .unwrap_err();
    assert_eq!(error.code, "AREPO_DUPLICATE");
}

#[tokio::test(flavor = "multi_thread")]
async fn profile_turns_adding_off() {
    let env = Env::with(&[], false).await;
    let error = probe(&env, "https://github.com/acme/agents")
        .await
        .unwrap_err();
    assert_eq!(error.code, "AREPO_ADD_DISABLED");
    assert_eq!(error.action, None);
    assert_eq!(CodedError::repair(&error), None);
}

#[tokio::test(flavor = "multi_thread")]
async fn remove_or_rename_a_company_repository() {
    let owner = unique_owner();
    let env = Env::new(&names(&owner, &["agents"])).await;
    commit_tree(&owner, "agents", &agents_repo(&["first"]));
    sync_one(&env).await;
    let id = repo_id(&env, "agents").await;
    assert_eq!(
        env.service.remove_repository(id).await.unwrap_err().code,
        "AREPO_REMOVE_NOT_OWN"
    );
    let renamed = env
        .service
        .rename_repository(RenameRepositoryInput {
            id,
            display_name: "Mine".into(),
        })
        .await;
    assert_eq!(renamed.unwrap_err().code, "AREPO_REMOVE_NOT_OWN");
}

#[tokio::test(flavor = "multi_thread")]
async fn no_read_access() {
    let env = Env::new(&[]).await;
    *env.github.permission.lock().unwrap() = RepoPermission::None;
    assert_eq!(
        probe(&env, "https://github.com/acme/secret")
            .await
            .unwrap_err()
            .code,
        "CAT_NO_READ_ACCESS"
    );
    *env.github.permission_error.lock().unwrap() = Some(GithubError::NotFound);
    assert_eq!(
        probe(&env, "https://github.com/acme/secret")
            .await
            .unwrap_err()
            .code,
        "CAT_NO_READ_ACCESS"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn github_sign_in_missing_or_expired() {
    let env = Env::new(&[]).await;
    *env.github.permission_error.lock().unwrap() = Some(GithubError::SignInNeeded);
    let error = probe(&env, "https://github.com/acme/agents")
        .await
        .unwrap_err();
    assert_eq!(error.code, "CAT_SIGN_IN_NEEDED");
    assert_eq!(error.action, Some(RepairAction::SignInAgain));
    // A fetch that needs the sign-in leaves the row unchanged and records it.
    let owner = unique_owner();
    let mut entry = Entry::ordin("agents", &owner, "agents");
    entry.signed = true;
    let env = Env::new(&[entry]).await;
    commit_tree(&owner, "agents", &agents_repo(&["first"]));
    *env.github.verified_error.lock().unwrap() = Some(GithubError::SignInNeeded);
    let report = env.service.sync_profile_repositories().await.unwrap();
    assert_eq!(
        report.results[0].outcome,
        RepositorySyncOutcome::Failed {
            code: "CAT_SIGN_IN_NEEDED".into()
        }
    );
    assert_eq!(
        last_result(&env, "agents").await,
        Some(FetchResult::SignInNeeded)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn no_format_plug_in_recognises_the_tree() {
    let env = Env::new(&[]).await;
    let owner = unique_owner();
    commit_tree(&owner, "notes", &tree_of(&[("README.md", "# Notes\n")]));
    let error = probe(&env, &format!("https://github.com/{owner}/notes"))
        .await
        .unwrap_err();
    assert_eq!(error.code, "CAT_FORMAT_UNKNOWN");
    let probes = env.dir.0.join("catalogue").join("probes");
    assert_eq!(
        std::fs::read_dir(probes).map(|d| d.count()).unwrap_or(0),
        0,
        "the probe folder is deleted"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn probe_unknown_or_expired() {
    let env = Env::new(&[]).await;
    let error = env
        .service
        .add_repository(AddRepositoryInput {
            probe_id: uuid::Uuid::now_v7(),
            display_name: "Agents".into(),
        })
        .await
        .unwrap_err();
    assert_eq!(error.code, "CAT_PROBE_EXPIRED");
}

#[tokio::test(flavor = "multi_thread")]
async fn display_name_empty_too_long_or_with_control_characters() {
    let env = Env::new(&[]).await;
    let owner = unique_owner();
    commit_tree(&owner, "agents", &agents_repo(&["first"]));
    for name in ["", "   ", &"x".repeat(121), "bad\u{7}name"] {
        let probed = probe(&env, &format!("https://github.com/{owner}/agents"))
            .await
            .unwrap();
        let error = env
            .service
            .add_repository(AddRepositoryInput {
                probe_id: probed.probe_id,
                display_name: name.to_owned(),
            })
            .await
            .unwrap_err();
        assert_eq!(error.code, "CAT_NAME_INVALID", "{name:?}");
    }
    let probed = probe(&env, &format!("https://github.com/{owner}/agents"))
        .await
        .unwrap();
    let added = env
        .service
        .add_repository(AddRepositoryInput {
            probe_id: probed.probe_id,
            display_name: "  Agents  ".into(),
        })
        .await
        .unwrap();
    assert_eq!(added.display_name, "Agents");
    let error = env
        .service
        .rename_repository(RenameRepositoryInput {
            id: added.id,
            display_name: String::new(),
        })
        .await
        .unwrap_err();
    assert_eq!(error.code, "CAT_NAME_INVALID");
    let renamed = env
        .service
        .rename_repository(RenameRepositoryInput {
            id: added.id,
            display_name: "Renamed".into(),
        })
        .await
        .unwrap();
    assert_eq!(renamed.display_name, "Renamed");
}

// Row: adding a repository when 10 besides the bundled catalogue are already read (CAT-LIM-1).
#[tokio::test(flavor = "multi_thread")]
async fn an_eleventh_repository_is_refused_until_one_is_removed() {
    let owner = unique_owner();
    let company: Vec<String> = (1..=9).map(|n| format!("company-{n}")).collect();
    let company: Vec<&str> = company.iter().map(String::as_str).collect();
    let env = Env::new(&names(&owner, &company)).await;
    // §B writes the nine profile rows; none of them exists to clone, so they stay `named` and count.
    env.service.sync_profile_repositories().await.unwrap();
    assert_eq!(
        env.count(
            "SELECT COUNT(*) FROM agent_repositories WHERE source = 'profile' \
             AND state IN ('named','cloned')"
        )
        .await,
        9
    );
    commit_tree(&owner, "mine", &agents_repo(&["first"]));
    commit_tree(&owner, "more", &agents_repo(&["second"]));
    let add = |probe_id, name: &str| AddRepositoryInput {
        probe_id,
        display_name: name.to_owned(),
    };

    // The tenth is accepted.
    let probed = probe(&env, &format!("https://github.com/{owner}/mine"))
        .await
        .unwrap();
    let mine = env
        .service
        .add_repository(add(probed.probe_id, "Mine"))
        .await
        .unwrap();

    // The eleventh is refused, nothing is written and the probe stays valid.
    let probed = probe(&env, &format!("https://github.com/{owner}/more"))
        .await
        .unwrap();
    let error = env
        .service
        .add_repository(add(probed.probe_id, "More agents"))
        .await
        .unwrap_err();
    assert_eq!(error.code, "CAT_REPOSITORY_LIMIT");
    assert!(!CodedError::retryable(&error));
    assert_eq!(CodedError::repair(&error), None);
    assert_eq!(
        env.count(&format!(
            "SELECT COUNT(*) FROM agent_repositories WHERE repository = '{owner}/more'"
        ))
        .await,
        0
    );

    // Removing the person's own repository makes room for the same probe.
    env.service.remove_repository(mine.id).await.unwrap();
    let more = env
        .service
        .add_repository(add(probed.probe_id, "More agents"))
        .await
        .unwrap();
    assert_eq!(more.display_name, "More agents");
}

#[tokio::test(flavor = "multi_thread")]
async fn repository_unreachable_network() {
    let owner = unique_owner();
    let env = Env::new(&names(&owner, &["gone"])).await;
    let report = env.service.sync_profile_repositories().await.unwrap();
    assert_eq!(
        report.results[0].outcome,
        RepositorySyncOutcome::Failed {
            code: "CAT_UNREACHABLE".into()
        }
    );
    assert_eq!(
        last_result(&env, "gone").await,
        Some(FetchResult::Unreachable)
    );
    // Retried at the next fetch: the repository appears and the fetch succeeds.
    commit_tree(&owner, "gone", &agents_repo(&["first"]));
    let outcomes = env.service.fetch_now(None).await.unwrap();
    assert!(matches!(outcomes[0].result, FetchResult::Ingested { .. }));
}

#[tokio::test(flavor = "multi_thread")]
async fn git_fetch_or_clone_failed_otherwise() {
    let owner = unique_owner();
    let mut entry = Entry::ordin("agents", &owner, "agents");
    entry.branch = "no-such-branch".to_owned();
    let env = Env::new(&[entry]).await;
    commit_tree(&owner, "agents", &agents_repo(&["first"]));
    let report = env.service.sync_profile_repositories().await.unwrap();
    assert_eq!(
        report.results[0].outcome,
        RepositorySyncOutcome::Failed {
            code: "CAT_FETCH_FAILED".into()
        }
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn merge_not_signed_where_required() {
    let owner = unique_owner();
    let mut entry = Entry::ordin("agents", &owner, "agents");
    entry.signed = true;
    let env = Env::new(&[entry]).await;
    commit_tree(&owner, "agents", &agents_repo(&["first"]));
    *env.github.verified.lock().unwrap() = false;
    env.service.sync_profile_repositories().await.unwrap();
    assert_eq!(
        last_result(&env, "agents").await,
        Some(FetchResult::MergeUnsigned)
    );
    assert!(*env.github.verified_calls.lock().unwrap() >= 1);
    // Once signed, the head is ingested.
    *env.github.verified.lock().unwrap() = true;
    let outcomes = env.service.fetch_now(None).await.unwrap();
    assert!(matches!(outcomes[0].result, FetchResult::Ingested { .. }));
}

#[tokio::test(flavor = "multi_thread")]
async fn withdrawn_list_unreadable() {
    let owner = unique_owner();
    let env = Env::new(&names(&owner, &["agents"])).await;
    let cases = [
        "listSchema: other/v9\nentries: []\n",
        "listSchema: ordin.withdrawn/v1\nentries:\n  - part: { kind: Skill, id: \"urn:ordin:acme:skills:x\", version: \"1.0.0\" }\n    reason: \"r\"\n    withdrawnAt: \"2026-01-10T09:00:00.000Z\"\n",
        "listSchema: ordin.withdrawn/v1\nentries:\n  - part: { kind: Skill, id: \"urn:ordin:acme:skills:x\" }\n    reason: \"\"\n    withdrawnAt: \"2026-01-10T09:00:00.000Z\"\n",
        "listSchema: ordin.withdrawn/v1\nentries:\n  - part: { kind: Skill, id: \"urn:ordin:acme:skills:x\" }\n    reason: \"r\"\n    withdrawnAt: \"2026-01-10T09:00:00.000Z\"\n    replacement: { id: \"urn:ordin:acme:skills:y\" }\n",
        "listSchema: ordin.withdrawn/v1\nentries:\n  - part: { kind: Skill, id: \"urn:ordin:acme:skills:x\" }\n    reason: \"r\"\n    withdrawnAt: \"yesterday\"\n",
        ": : not yaml",
    ];
    for text in cases {
        commit_tree(
            &owner,
            "agents",
            &with(agents_repo(&["first"]), "withdrawn.yaml", text),
        );
        let outcomes = env.service.sync_profile_repositories().await.unwrap();
        let _ = outcomes;
        assert_eq!(
            last_result(&env, "agents").await,
            Some(FetchResult::WithdrawnListUnreadable),
            "{text}"
        );
    }
    let row = &env.service.list_repositories().await.unwrap()[0];
    assert_eq!(
        row.repository.last_fetched_commit, None,
        "a list that was never read leaves nothing ingested"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn commit_refused_format_changed() {
    let owner = unique_owner();
    let env = Env::new(&names(&owner, &["agents"])).await;
    commit_tree(&owner, "agents", &agents_repo(&["first"]));
    sync_one(&env).await;
    commit_tree(
        &owner,
        "agents",
        &tree_of(&[("README.md", "no agents any more\n")]),
    );
    let outcomes = env.service.fetch_now(None).await.unwrap();
    assert_eq!(
        outcomes[0].result,
        FetchResult::CommitRefused {
            reason_code: "CAT_FORMAT_CHANGED".into()
        }
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn commit_refused_repository_too_large() {
    let owner = unique_owner();
    let env = Env::new(&names(&owner, &["agents"])).await;
    let mut files = agents_repo(&["first"]);
    for n in 0..2001 {
        let (path, text) = rule_file(&format!("many-{n}"), "");
        files.insert(path, text.into_bytes());
    }
    commit_tree(&owner, "agents", &files);
    env.service.sync_profile_repositories().await.unwrap();
    assert_eq!(
        last_result(&env, "agents").await,
        Some(FetchResult::CommitRefused {
            reason_code: "CAT_REPOSITORY_TOO_LARGE".into()
        })
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn commit_refused_part_duplicate() {
    let owner = unique_owner();
    let env = Env::new(&names(&owner, &["agents"])).await;
    let (path, text) = rule_file("tone", "");
    let mut files = agents_repo(&["first"]);
    files.insert(path.replace("tone", "tone-again"), text.into_bytes());
    commit_tree(&owner, "agents", &files);
    env.service.sync_profile_repositories().await.unwrap();
    assert_eq!(
        last_result(&env, "agents").await,
        Some(FetchResult::CommitRefused {
            reason_code: "PART_DUPLICATE".into()
        })
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn commit_refused_checker_unavailable() {
    let owner = unique_owner();
    let env = Env::unbound(&names(&owner, &["agents"])).await;
    commit_tree(&owner, "agents", &agents_repo(&["first"]));
    env.service.sync_profile_repositories().await.unwrap();
    assert_eq!(
        last_result(&env, "agents").await,
        Some(FetchResult::CommitRefused {
            reason_code: "CAT_CHECK_UNAVAILABLE".into()
        })
    );
    let row = &env.service.list_repositories().await.unwrap()[0];
    assert_eq!(
        row.repository.last_fetched_commit, None,
        "retried at the next tick"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn datastore_failure() {
    let env = Env::new(&[]).await;
    sqlx::query("DROP TABLE agent_repositories CASCADE")
        .execute(env.pool())
        .await
        .ok();
    sqlx::query("ALTER TABLE agent_repositories RENAME TO gone")
        .execute(env.pool())
        .await
        .unwrap();
    let error = env.service.list_repositories().await.unwrap_err();
    assert_eq!(error.code, "CAT_STORE_FAILED");
    assert!(error.retryable);
    assert_eq!(error.action, Some(RepairAction::TryAgain));
}

#[tokio::test(flavor = "multi_thread")]
async fn unknown_part_repository_or_lifecycle() {
    let env = Env::new(&[]).await;
    let part = ordin_types::PartId::new();
    assert_eq!(
        env.service.part(part).await.unwrap_err().code,
        "CAT_PART_UNKNOWN"
    );
    assert_eq!(
        env.service.read_part(part).await.unwrap_err().code,
        "CAT_PART_UNKNOWN"
    );
    assert_eq!(
        env.service.used_by(part).await.unwrap_err().code,
        "CAT_PART_UNKNOWN"
    );
    assert_eq!(
        env.service.made_of(part).await.unwrap_err().code,
        "CAT_PART_UNKNOWN"
    );
    assert_eq!(
        env.service.part_history(part).await.unwrap_err().code,
        "CAT_PART_UNKNOWN"
    );
    let repository = AgentRepositoryId::new();
    assert_eq!(
        env.service.health(repository).await.unwrap_err().code,
        "CAT_REPOSITORY_UNKNOWN"
    );
    assert_eq!(
        env.service
            .withdrawn_list(repository)
            .await
            .unwrap_err()
            .code,
        "CAT_REPOSITORY_UNKNOWN"
    );
    assert_eq!(
        env.service.vocabulary(repository).await.unwrap_err().code,
        "CAT_REPOSITORY_UNKNOWN"
    );
    assert_eq!(
        env.service.layout(repository).await.unwrap_err().code,
        "CAT_REPOSITORY_UNKNOWN"
    );
    assert_eq!(
        env.service
            .fetch_now(Some(repository))
            .await
            .unwrap_err()
            .code,
        "CAT_REPOSITORY_UNKNOWN"
    );
    assert_eq!(
        env.service
            .repository_ref(repository)
            .await
            .unwrap_err()
            .code,
        "CAT_REPOSITORY_UNKNOWN"
    );
    assert_eq!(
        env.service
            .lifecycle(ordin_types::LifecycleId::new())
            .await
            .unwrap_err()
            .code,
        "CAT_LIFECYCLE_UNKNOWN"
    );
    assert_eq!(
        env.service
            .lifecycle_place(ordin_types::HandoverPlaceId::new())
            .await
            .unwrap_err()
            .code,
        "CAT_LIFECYCLE_UNKNOWN"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn search_text_over_500_chars() {
    let env = Env::new(&[]).await;
    let query = LibraryQuery {
        text: Some("x".repeat(501)),
        ..LibraryQuery::default()
    };
    assert_eq!(
        env.service.search(query.clone()).await.unwrap_err().code,
        "CAT_QUERY_INVALID"
    );
    assert_eq!(
        env.service.facets(query).await.unwrap_err().code,
        "CAT_QUERY_INVALID"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn malformed_cursor() {
    let env = Env::new(&[]).await;
    let query = LibraryQuery {
        cursor: Some(Cursor::encode(&1)),
        ..LibraryQuery::default()
    };
    assert_eq!(
        env.service.search(query).await.unwrap_err().code,
        "SRV_BAD_CURSOR"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn requirements_list_empty_over_50_or_an_item_over_2000_chars() {
    let env = Env::new(&[]).await;
    for requirements in [
        Vec::new(),
        (0..51).map(|n| n.to_string()).collect(),
        vec!["x".repeat(2001)],
    ] {
        let error = env
            .service
            .match_requirements(MatchRequirementsInput { requirements })
            .await
            .unwrap_err();
        assert_eq!(error.code, "CAT_REQUIREMENTS_INVALID");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_profile_entrys_key_is_ordin_reserved_for_the_bundled_catalogue() {
    let owner = unique_owner();
    let reserved = Entry::ordin("ordin", &owner, "agents");
    let env = Env::new(&[reserved, Entry::ordin("fine", &owner, "other")]).await;
    commit_tree(&owner, "other", &agents_repo(&["first"]));
    let report = env.service.sync_profile_repositories().await.unwrap();
    assert_eq!(
        report.results.len(),
        1,
        "the entry is skipped, the others continue"
    );
    assert_eq!(
        env.count("SELECT COUNT(*) FROM agent_repositories WHERE repository LIKE '%/agents'")
            .await,
        0
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn sync_profile_repositories_one_repositorys_clone_fetch_or_ingest_fails() {
    let owner = unique_owner();
    let env = Env::new(&names(&owner, &["bad", "good"])).await;
    commit_tree(&owner, "bad", &tree_of(&[("README.md", "not agents\n")]));
    commit_tree(&owner, "good", &agents_repo(&["first"]));
    let report = env
        .service
        .sync_profile_repositories()
        .await
        .expect("the call itself succeeds");
    let outcomes: Vec<_> = report.results.iter().map(|r| r.outcome.clone()).collect();
    assert_eq!(
        outcomes[1],
        RepositorySyncOutcome::Cloned,
        "the others continue"
    );
    // A commit refused for its content is not a failure of the fetch: the clone is current.
    assert_eq!(outcomes[0], RepositorySyncOutcome::Cloned);
}

#[tokio::test(flavor = "multi_thread")]
async fn repository_by_ref_with_a_malformed_address() {
    let env = Env::new(&[]).await;
    let error = env
        .service
        .repository_by_ref(&AgentRepositoryRef::Address(
            "https://github.com/only".into(),
        ))
        .await
        .unwrap_err();
    assert_eq!(error.code, "AREPO_NAME_INVALID");
    let error = env
        .service
        .repository_by_ref(&AgentRepositoryRef::Address("nonsense".into()))
        .await
        .unwrap_err();
    assert_eq!(error.code, "AREPO_NAME_INVALID");
}

#[tokio::test(flavor = "multi_thread")]
async fn find_part_with_no_matching_head_part() {
    let owner = unique_owner();
    let env = Env::new(&names(&owner, &["agents"])).await;
    commit_tree(&owner, "agents", &agents_repo(&["first"]));
    sync_one(&env).await;
    let id = repo_id(&env, "agents").await;
    let error = env
        .service
        .find_part(id, PartKind::Skill, "urn:ordin:acme:skills:none", None)
        .await
        .unwrap_err();
    assert_eq!(error.code, "CAT_PART_UNKNOWN");
    let error = env
        .service
        .find_part(AgentRepositoryId::new(), PartKind::Skill, "x", None)
        .await
        .unwrap_err();
    assert_eq!(error.code, "CAT_REPOSITORY_UNKNOWN");
}

#[tokio::test(flavor = "multi_thread")]
async fn worktree_requested_without_review_right() {
    let owner = unique_owner();
    let env = Env::new(&names(&owner, &["agents"])).await;
    commit_tree(&owner, "agents", &agents_repo(&["first"]));
    *env.github.permission.lock().unwrap() = RepoPermission::Read;
    sync_one(&env).await;
    let id = repo_id(&env, "agents").await;
    let error = env
        .service
        .open_draft_worktree(DraftWorktreeInput {
            agent_repository_id: id,
            branch: "draft/one".into(),
        })
        .await
        .unwrap_err();
    assert_eq!(error.code, "CAT_NO_REVIEW_RIGHT");
}

#[tokio::test(flavor = "multi_thread")]
async fn draft_branch_already_exists() {
    let owner = unique_owner();
    let env = Env::new(&names(&owner, &["agents"])).await;
    commit_tree(&owner, "agents", &agents_repo(&["first"]));
    sync_one(&env).await;
    let id = repo_id(&env, "agents").await;
    let opened = env
        .service
        .open_draft_worktree(DraftWorktreeInput {
            agent_repository_id: id,
            branch: "draft/one".into(),
        })
        .await
        .unwrap();
    assert!(opened.path.join("models").exists());
    let error = env
        .service
        .open_draft_worktree(DraftWorktreeInput {
            agent_repository_id: id,
            branch: "draft/one".into(),
        })
        .await
        .unwrap_err();
    assert_eq!(error.code, "CAT_BRANCH_EXISTS");
    let error = env
        .service
        .open_draft_worktree(DraftWorktreeInput {
            agent_repository_id: id,
            branch: "bad..name".into(),
        })
        .await
        .unwrap_err();
    assert_eq!(error.code, "AREPO_BRANCH_INVALID");
    env.service
        .close_draft_worktree("draft/one", id)
        .await
        .unwrap();
    assert!(!opened.path.exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn partsource_identifier_not_found_or_refused_at_ingest() {
    let owner = unique_owner();
    let env = Env::new(&names(&owner, &["agents"])).await;
    commit_tree(&owner, "agents", &agents_repo(&["first"]));
    sync_one(&env).await;
    let id = repo_id(&env, "agents").await;
    let scope = env
        .service
        .source_scope(RepositoryRef::Repository(id))
        .await
        .unwrap();
    let found = env
        .service
        .resolve(&scope, PartKind::Skill, "urn:ordin:acme:skills:none", None)
        .await
        .unwrap();
    assert!(matches!(found, ordin_agent_model::Resolution::NotFound));
    let found = env
        .service
        .resolve_any_kind(&scope, "urn:ordin:acme:skills:none")
        .await
        .unwrap();
    assert!(matches!(found, ordin_agent_model::Resolution::NotFound));
}

#[tokio::test(flavor = "multi_thread")]
async fn partsource_index_or_clone_unreadable() {
    let owner = unique_owner();
    let env = Env::new(&names(&owner, &["agents"])).await;
    commit_tree(&owner, "agents", &agents_repo(&["first"]));
    sync_one(&env).await;
    let id = repo_id(&env, "agents").await;
    let scope = env
        .service
        .source_scope(RepositoryRef::Repository(id))
        .await
        .unwrap();
    sqlx::query("ALTER TABLE part_index RENAME TO part_index_gone")
        .execute(env.pool())
        .await
        .unwrap();
    let error = env
        .service
        .resolve(&scope, PartKind::Rule, TONE_ID, None)
        .await
        .unwrap_err();
    assert!(
        matches!(error, PartSourceError::Unavailable { retryable: true }),
        "{error:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn partsource_a_repositorys_withdrawn_list_never_read() {
    let owner = unique_owner();
    let env = Env::new(&names(&owner, &["agents"])).await;
    commit_tree(&owner, "agents", &agents_repo(&["first"]));
    // The row is named, never fetched.
    env.service.sync_profile_repositories().await.unwrap();
    sqlx::query("UPDATE agent_repositories SET last_fetched_commit = NULL")
        .execute(env.pool())
        .await
        .unwrap();
    let id = repo_id(&env, "agents").await;
    let scope = SourceScope {
        home: RepositoryRef::Repository(id),
        profile: Vec::new(),
        include_bundled: false,
    };
    let error = env.service.withdrawn(&scope).await.unwrap_err();
    match error {
        PartSourceError::WithdrawnListUnreadable { repository } => {
            assert!(repository.ends_with("/agents"))
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn partsource_a_stored_parts_file_is_gone_at_its_commit() {
    let owner = unique_owner();
    let env = Env::new(&names(&owner, &["agents"])).await;
    commit_tree(&owner, "agents", &agents_repo(&["first"]));
    sync_one(&env).await;
    let id = repo_id(&env, "agents").await;
    let scope = env
        .service
        .source_scope(RepositoryRef::Repository(id))
        .await
        .unwrap();
    sqlx::query("UPDATE parts SET \"commit\" = '0123456789012345678901234567890123456789' WHERE identifier = ?1")
        .bind(TONE_ID)
        .execute(env.pool())
        .await
        .unwrap();
    let error = env
        .service
        .resolve(&scope, PartKind::Rule, TONE_ID, None)
        .await
        .unwrap_err();
    assert!(
        matches!(error, PartSourceError::Corrupt { part_identifier } if part_identifier == TONE_ID)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn source_scope_for_an_unknown_repository() {
    let env = Env::new(&[]).await;
    let error = env
        .service
        .source_scope(RepositoryRef::Repository(AgentRepositoryId::new()))
        .await
        .unwrap_err();
    assert_eq!(error.code, "CAT_REPOSITORY_UNKNOWN");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_repository_part_of_dot_or_dot_dot_is_refused() {
    let env = Env::new(&[]).await;
    for address in [
        "https://github.com/../..",
        "https://github.com/owner/..",
        "https://github.com/./name",
        "https://github.com/owner/...git",
    ] {
        let error = probe(&env, address).await.unwrap_err();
        assert_eq!(error.code, "AREPO_NAME_INVALID", "{address}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn closing_a_draft_worktree_refuses_an_invalid_branch() {
    let env = Env::new(&[]).await;
    for branch in ["..", "bad..name", ""] {
        let error = env
            .service
            .close_draft_worktree(branch, AgentRepositoryId::new())
            .await
            .unwrap_err();
        assert_eq!(error.code, "AREPO_BRANCH_INVALID", "{branch:?}");
    }
}
