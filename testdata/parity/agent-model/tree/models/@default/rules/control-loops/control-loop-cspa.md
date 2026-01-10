---
apiVersion: ai.enterprise/v1
kind: control-loop
metadata:
  id: urn:ordin:default:rules:control-loops:cspa
  name: Evidence first loop
  version: 1.2.0
  owner: engineering-enablement
modes:
  - urn:ordin:default:rules:modes:review
requires:
  - evidence
strategy: observe-then-act
trigger: every-task
---
Observe first. Then act.
