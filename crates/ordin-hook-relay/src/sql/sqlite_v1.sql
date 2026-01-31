-- spec: ordin-desktop/components/HookRelay#SA-RECON-04
CREATE TABLE hook_late_decisions (
    approval_id TEXT PRIMARY KEY CHECK (length(approval_id) = 36),
    run_id TEXT NOT NULL CHECK (length(run_id) = 36),
    kind TEXT NOT NULL CHECK (kind IN ('tool_call', 'stop_gate')),
    created_at TEXT NOT NULL
);
