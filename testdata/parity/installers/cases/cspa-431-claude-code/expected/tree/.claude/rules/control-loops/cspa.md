---
apiVersion: ai.enterprise/v1
kind: control-loop
metadata:
  id: urn:ordin:default:rules:control-loops:cspa
  name: cspa
  version: 1.0.0
  owner: engineering-enablement
modes:
  - urn:ordin:default:rules:modes:plan-mode
  - urn:ordin:default:rules:modes:build-mode
requires:
  - bounded-iteration
strategy: plan-then-execute
trigger: human-pulled
---
Plan first, then build what the plan says.
