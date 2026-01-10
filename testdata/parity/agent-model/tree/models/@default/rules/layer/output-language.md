---
apiVersion: ai.enterprise/v1
kind: rule-layer
metadata:
  id: urn:ordin:default:rules:layer:output-language
  name: Output language
  version: 1.0.0
  owner: engineering-enablement
  loading: always-on
  description: Writes every narrative in the active locale.
  category: writing
---
Write in British English unless a language overlay says otherwise.
