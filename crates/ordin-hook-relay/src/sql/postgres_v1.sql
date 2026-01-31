-- spec: ordin-desktop/components/HookRelay#SA-RECON-04
CREATE TABLE hook_late_decisions (
    approval_id uuid PRIMARY KEY,
    run_id uuid NOT NULL,
    kind text NOT NULL CHECK (kind IN ('tool_call', 'stop_gate')),
    created_at timestamptz NOT NULL
);
