---
apiVersion: ai.enterprise/v1
kind: control-loop
metadata:
  id: urn:ordin:default:rules:control-loops:respond
  name: respond
  version: 1.0.0
  owner: support-operations
strategy: reactive
trigger: autonomous
---
Answer each question as it arrives.
