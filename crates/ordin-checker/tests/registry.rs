//! X4: every code in `ALL_CODES` is registered, and its prefix is owned by this crate
//! (ErrorHandling.md *Internal Logic* 11).

use ordin_checker::ALL_CODES;
use ordin_types::error_registry::{lookup, owns_prefix};

#[test]
fn every_code_is_registered_and_owned() {
    assert_eq!(ALL_CODES.len(), 12);
    for code in ALL_CODES {
        let entry = lookup(code).unwrap_or_else(|| panic!("{code} is not registered"));
        assert_eq!(entry.owner_crate, "ordin-checker", "{code}");
        let prefix = code.split('_').next().expect("a prefix");
        assert!(owns_prefix("ordin-checker", prefix), "{prefix}");
    }
}

#[test]
fn every_error_variant_code_is_in_all_codes() {
    use ordin_checker::{CheckError, CheckRunId};
    use ordin_types::AgentInstanceId;
    let errors = [
        CheckError::SourceUnavailable { retryable: true },
        CheckError::ResolverFailed {
            code: "RES_SOURCE_UNAVAILABLE".to_owned(),
            retryable: true,
        },
        CheckError::InputInvalid { field: "job_tasks" },
        CheckError::TooManyFindings,
        CheckError::RunNotFound {
            id: CheckRunId(uuid::Uuid::now_v7()),
        },
        CheckError::DatastoreFailed { retryable: false },
        CheckError::SourceNotBound,
        CheckError::SourceAlreadyBound,
        CheckError::ContractInvalid {
            reason: "no_sections",
        },
        CheckError::InstanceUnknown {
            id: AgentInstanceId::new(),
        },
    ];
    for error in errors {
        assert!(ALL_CODES.contains(&error.code()), "{}", error.code());
        assert_eq!(error.to_string(), error.code());
    }
}
