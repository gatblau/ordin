---
apiVersion: ai.enterprise/v1
kind: control-loop
metadata:
  id: urn:ordin:acme:rules:control-loops:loop-a
  name: loop-a
  version: 1.0.0
  owner: legal-ops
requires:
  - evidence
strategy: observe-then-act
trigger: every-task
---
Observe first.
