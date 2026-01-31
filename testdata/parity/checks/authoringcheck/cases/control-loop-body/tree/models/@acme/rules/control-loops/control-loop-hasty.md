---
apiVersion: ai.enterprise/v1
kind: control-loop
metadata:
  id: urn:ordin:acme:rules:control-loops:hasty
  name: hasty
  version: 1.0.0
  owner: legal-ops
requires:
  - teleport
strategy: guess-and-hope
trigger: Whenever!
---
Act at once.
