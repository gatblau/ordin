//! C8 — `GET /openapi.json`. A hand-built OpenAPI 3.1 document enumerating the public routes
//! (SA-VER-01). It is the contract document, so it is NOT wrapped in the success envelope and is served
//! with `content-type: application/json`. The document is deliberately hand-authored rather than
//! derived from the handler types (a documented C8 deviation / follow-up); it enumerates each route,
//! its method, its operation id, and its success status.

use serde_json::{json, Value};

/// Build the OpenAPI 3.1 document for the `recall` HTTP surface.
pub fn document(service: &str, version: &str) -> Value {
    json!({
        "openapi": "3.1.0",
        "info": {
            "title": service,
            "version": version,
            "description": "recall — agentic memory service HTTP API"
        },
        "paths": {
            "/v1": {
                "get": {
                    "operationId": "capabilities",
                    "summary": "Service capabilities",
                    "responses": { "200": { "description": "Success<Capabilities>" } }
                }
            },
            "/v1/recall": {
                "post": {
                    "operationId": "recall",
                    "summary": "Recall ranked facts (synchronous read path)",
                    "requestBody": {
                        "required": true,
                        "content": { "application/json": { "schema": recall_request_schema() } }
                    },
                    "responses": {
                        "200": { "description": "Success<RecallResponse>" },
                        "400": { "description": "VAL_INVALID_BODY | VAL_OUT_OF_RANGE | VAL_UNSUPPORTED_CLASS" },
                        "401": { "description": "AUTH_MISSING_TOKEN | AUTH_INVALID_TOKEN" },
                        "403": { "description": "AUTH_INSUFFICIENT_SCOPE" },
                        "413": { "description": "VAL_BODY_TOO_LARGE" },
                        "429": { "description": "RATE_LIMITED" }
                    }
                }
            },
            "/v1/memories": {
                "post": {
                    "operationId": "remember",
                    "summary": "Enqueue a fact-extraction job (async)",
                    "requestBody": {
                        "required": true,
                        "content": { "application/json": { "schema": remember_request_schema() } }
                    },
                    "responses": {
                        "202": { "description": "Success<WriteAck>" },
                        "400": { "description": "VAL_INVALID_BODY | VAL_MISSING_IDEMPOTENCY_KEY" },
                        "503": { "description": "QUEUE_UNAVAILABLE" }
                    }
                }
            },
            "/v1/memories/{id}": {
                "get": {
                    "operationId": "get_fact",
                    "summary": "Fetch a fact by id (conditional, ETag)",
                    "responses": {
                        "200": { "description": "Success<Fact>" },
                        "304": { "description": "Not Modified" },
                        "404": { "description": "NOT_FOUND" }
                    }
                },
                "delete": {
                    "operationId": "delete",
                    "summary": "Verifiable hard delete (returns a deletion proof)",
                    "responses": {
                        "200": { "description": "Success<DeletionProof>" },
                        "404": { "description": "NOT_FOUND" }
                    }
                }
            },
            "/v1/memories/{id}/retire": {
                "post": {
                    "operationId": "retire",
                    "summary": "End a fact's validity (non-destructive)",
                    "responses": {
                        "200": { "description": "Success<RetireAck>" },
                        "404": { "description": "NOT_FOUND" }
                    }
                }
            },
            "/v1/memories/{id}/history": {
                "get": {
                    "operationId": "fact_history",
                    "summary": "A fact's versions, oldest first, with the links each asserted or ended",
                    "responses": {
                        "200": { "description": "Success<FactHistory>" },
                        "404": { "description": "NOT_FOUND" }
                    }
                }
            },
            "/v1/memories/{id}/origins": {
                "get": {
                    "operationId": "fact_origins",
                    "summary": "The facts a summary was made from",
                    "responses": {
                        "200": { "description": "Success<Vec<OriginFact>>" },
                        "404": { "description": "NOT_FOUND" }
                    }
                }
            },
            "/v1/entities": {
                "get": {
                    "operationId": "list_entities",
                    "summary": "The caller's entities by name, with fact and link counts",
                    "parameters": [
                        { "name": "search", "in": "query", "schema": { "type": "string", "minLength": 1, "maxLength": 200 } },
                        { "name": "cursor", "in": "query", "schema": { "type": "string" } }
                    ],
                    "responses": {
                        "200": { "description": "Success<EntityPage>" },
                        "400": { "description": "VAL_OUT_OF_RANGE" }
                    }
                }
            },
            "/v1/entities/{id}": {
                "get": {
                    "operationId": "entity_detail",
                    "summary": "One entity with its facts and links",
                    "responses": {
                        "200": { "description": "Success<EntityDetail>" },
                        "404": { "description": "NOT_FOUND" }
                    }
                }
            },
            "/openapi.json": {
                "get": {
                    "operationId": "openapi",
                    "summary": "This OpenAPI document",
                    "responses": { "200": { "description": "OpenAPI 3.1 JSON" } }
                }
            }
        }
    })
}

/// The `RecallRequest` body schema (2C.4), as the MCP edge advertises it for its `recall` tool, so the
/// REST and MCP shapes do not diverge (ADR-020): `query` is no longer required, and exactly one of
/// `query` and `searches` is stated in the description and enforced by C6.
fn recall_request_schema() -> Value {
    json!({
        "type": "object",
        "description": "Exactly one of `query` and `searches` (ADR-020).",
        "properties": {
            "query": { "type": "string", "description": "1..=4096 chars; exactly one of query and searches." },
            "searches": {
                "type": "array",
                "minItems": 1,
                "maxItems": 4,
                "description": "1..=4 typed sub-queries, the first weighted double; exactly one of query and searches.",
                "items": {
                    "type": "object",
                    "properties": {
                        "type": { "type": "string", "enum": ["lex", "vec"] },
                        "query": { "type": "string", "minLength": 1, "maxLength": 4096 }
                    },
                    "required": ["type", "query"]
                }
            },
            "filters": {
                "type": "object",
                "properties": {
                    "memory_class": { "type": "string" },
                    "visibility": { "type": "string" },
                    "entity": { "type": "string" },
                    "valid_at": { "type": "string", "format": "date-time" },
                    "collections": {
                        "type": "array",
                        "minItems": 1,
                        "maxItems": 32,
                        "uniqueItems": true,
                        "items": { "type": "string", "pattern": "^[a-z0-9:_-]{0,64}$" }
                    }
                }
            },
            "result_cap": { "type": "integer", "minimum": 1, "maximum": 50, "default": 10 },
            "cursor": { "type": "string" },
            "include_provenance": { "type": "boolean", "default": false }
        }
    })
}

/// The `RememberRequest` body schema (2C.4), as the MCP edge advertises it (the idempotency key is a
/// header here, not a body field).
fn remember_request_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "content": { "type": "object", "description": "Structured assertion object (ADR-015)." },
            "source": {
                "type": "object",
                "properties": {
                    "origin_ref": { "type": "string" },
                    "modification_marker": { "type": "string" }
                },
                "required": ["origin_ref"]
            },
            "memory_class": { "type": "string" },
            "collection": { "type": "string", "pattern": "^[a-z0-9:_-]{0,64}$" },
            "entities": entities_schema(),
            "links": links_schema(),
            "derived_from": derived_from_schema()
        },
        "required": ["content"]
    })
}

/// `RememberRequest.entities` (ADR-021), shared with the MCP edge's `remember` schema.
pub fn entities_schema() -> Value {
    json!({
        "type": "array",
        "maxItems": 16,
        "description": "The things the fact is about.",
        "items": {
            "type": "object",
            "properties": {
                "name": { "type": "string", "minLength": 1, "maxLength": 512 },
                "aliases": {
                    "type": "array",
                    "maxItems": 32,
                    "items": { "type": "string", "minLength": 1, "maxLength": 512 }
                }
            },
            "required": ["name"],
            "additionalProperties": false
        }
    })
}

/// `RememberRequest.links` (ADR-021).
pub fn links_schema() -> Value {
    json!({
        "type": "array",
        "maxItems": 16,
        "description": "Links between the things the request names; set ended for a link that is no longer true.",
        "items": {
            "type": "object",
            "properties": {
                "from": { "type": "string" },
                "kind": { "type": "string", "minLength": 1, "maxLength": 128 },
                "to": { "type": "string" },
                "ended": { "type": "boolean", "default": false },
                "confidence": { "type": "number", "minimum": 0, "maximum": 1 }
            },
            "required": ["from", "kind", "to"],
            "additionalProperties": false
        }
    })
}

/// `RememberRequest.derived_from` (ADR-021).
pub fn derived_from_schema() -> Value {
    json!({
        "type": "array",
        "minItems": 2,
        "maxItems": 64,
        "uniqueItems": true,
        "description": "The facts a consolidated fact summarises (memory_class consolidated only).",
        "items": { "type": "string", "pattern": "^fact:" }
    })
}
