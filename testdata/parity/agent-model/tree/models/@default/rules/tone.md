---
apiVersion: ai.enterprise/v1
kind: rule
metadata:
  id: urn:ordin:default:rules:tone
  name: Tone
  version: 2.1.0
  owner: engineering-enablement
  description: Plain and direct wording.
  category: writing
  audience:
    domains:
      - finance
    roles:
      - analyst
      - reviewer
    languages:
      - en-GB
  useCases:
    - title: Supplier letters
      summary: Letters to suppliers stay short.
  status: deprecated
  supersedes: urn:ordin:default:rules:old-tone
  tags:
    - style
    - "1.0"
---
Use short sentences.
