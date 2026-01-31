//! The pure conversions (Internal Logic 8) and the address and query rules (Internal Logic 2.1, 3).

use serde_json::{json, Value};

use crate::query::{build, quote, Built};
use crate::{JiraAdapter, JiraCodeBlock, JiraError, JiraQuery};

fn adf(markdown: &str) -> Value {
    JiraAdapter::markdown_to_adf(markdown, None).unwrap()
}

fn content(doc: &Value) -> &Vec<Value> {
    doc["content"].as_array().unwrap()
}

fn md(doc: Value) -> String {
    JiraAdapter::adf_to_markdown(&doc).unwrap().0
}

fn doc_of(nodes: Value) -> Value {
    json!({"version": 1, "type": "doc", "content": nodes})
}

#[test]
fn markdown_blocks_map_to_adf_nodes() {
    let doc = adf("# Title\n\nSome *em*, **strong**, ~~gone~~ and `code`.\n\n> quoted\n\n---\n");
    let nodes = content(&doc);
    assert_eq!(doc["version"], 1);
    assert_eq!(nodes[0]["type"], "heading");
    assert_eq!(nodes[0]["attrs"]["level"], 1);
    let marks: Vec<&str> = nodes[1]["content"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|n| n["marks"][0]["type"].as_str())
        .collect();
    assert_eq!(marks, ["em", "strong", "strike", "code"]);
    assert_eq!(nodes[2]["type"], "blockquote");
    assert_eq!(nodes[3]["type"], "rule");
}

#[test]
fn markdown_lists_become_list_items_with_paragraphs() {
    let doc = adf("- one\n- two\n\n3. three\n4. four\n");
    let nodes = content(&doc);
    assert_eq!(nodes[0]["type"], "bulletList");
    assert_eq!(nodes[0]["content"][0]["type"], "listItem");
    assert_eq!(nodes[0]["content"][1]["content"][0]["type"], "paragraph");
    assert_eq!(nodes[1]["type"], "orderedList");
    assert_eq!(nodes[1]["attrs"]["order"], 3);
}

#[test]
fn only_https_http_and_mailto_links_keep_their_mark() {
    let doc = adf("[a](https://x.example/a) [b](javascript:alert(1)) [c](mailto:a@b.example)");
    let text: Vec<&Value> = content(&doc)[0]["content"]
        .as_array()
        .unwrap()
        .iter()
        .collect();
    let hrefs: Vec<Option<&str>> = text
        .iter()
        .map(|n| n["marks"][0]["attrs"]["href"].as_str())
        .collect();
    assert!(hrefs.contains(&Some("https://x.example/a")));
    assert!(hrefs.contains(&Some("mailto:a@b.example")));
    assert!(!hrefs.iter().flatten().any(|h| h.starts_with("javascript")));
    let plain = text
        .iter()
        .find(|n| n["text"].as_str().is_some_and(|t| t.contains('b')))
        .expect("the text of the refused link stays");
    assert!(plain.get("marks").is_none());
}

#[test]
fn a_fenced_block_keeps_its_language_and_text() {
    let doc = adf("```yaml\nkind: sent\n```\n");
    let block = &content(&doc)[0];
    assert_eq!(block["type"], "codeBlock");
    assert_eq!(block["attrs"]["language"], "yaml");
    assert_eq!(block["content"][0]["text"], "kind: sent");
}

#[test]
fn images_and_tables_become_their_source_text() {
    let doc = adf("![alt](https://x.example/i.png)");
    assert_eq!(
        content(&doc)[0]["content"][0]["text"],
        "![alt](https://x.example/i.png)"
    );
    let doc = adf("| a | b |\n| - | - |\n| 1 | 2 |");
    let text = content(&doc)[0]["content"][0]["text"].as_str().unwrap();
    assert!(text.starts_with("| a | b |"));
}

#[test]
fn an_empty_markdown_still_gives_a_document_with_one_paragraph() {
    let doc = adf("");
    assert_eq!(content(&doc).len(), 1);
    assert_eq!(content(&doc)[0]["type"], "paragraph");
}

#[test]
fn the_code_block_is_appended_exactly_as_given() {
    let block = JiraCodeBlock {
        language: Some("yaml".into()),
        text: "# ordin.handover/v1 event\nkind: sent\n".into(),
    };
    let doc = JiraAdapter::markdown_to_adf("hello", Some(&block)).unwrap();
    let last = content(&doc).last().unwrap();
    assert_eq!(last["attrs"]["language"], "yaml");
    assert_eq!(last["content"][0]["text"], block.text.as_str());
    let (_, blocks) = JiraAdapter::adf_to_markdown(&doc).unwrap();
    assert_eq!(blocks, vec![block]);
}

#[test]
fn a_block_without_text_or_language_has_neither() {
    let block = JiraCodeBlock {
        language: None,
        text: String::new(),
    };
    let doc = JiraAdapter::markdown_to_adf("x", Some(&block)).unwrap();
    let last = content(&doc).last().unwrap();
    assert!(last.get("attrs").is_none() && last.get("content").is_none());
}

#[test]
fn adf_nodes_read_back_as_markdown() {
    let doc = doc_of(json!([
        {"type": "heading", "attrs": {"level": 2}, "content": [{"type": "text", "text": "Plan"}]},
        {"type": "paragraph", "content": [
            {"type": "text", "text": "see "},
            {"type": "text", "text": "docs", "marks": [{"type": "link", "attrs": {"href": "https://x.example"}}]},
            {"type": "text", "text": " now ", "marks": [{"type": "strong"}]},
            {"type": "mention", "attrs": {"id": "1", "text": "@Sam Lee"}},
            {"type": "emoji", "attrs": {"shortName": ":smile:", "text": "x"}},
            {"type": "status", "attrs": {"text": "In Progress"}},
            {"type": "date", "attrs": {"timestamp": "1768003200000"}},
            {"type": "inlineCard", "attrs": {"url": "https://x.example/c"}}
        ]},
        {"type": "bulletList", "content": [
            {"type": "listItem", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "a"}]}]}
        ]},
        {"type": "orderedList", "attrs": {"order": 2}, "content": [
            {"type": "listItem", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "b"}]}]}
        ]},
        {"type": "rule"},
        {"type": "blockquote", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "q"}]}]},
        {"type": "panel", "attrs": {"panelType": "info"}, "content": [{"type": "paragraph", "content": [{"type": "text", "text": "p"}]}]},
        {"type": "blockCard", "attrs": {"url": "https://x.example/b"}},
        {"type": "mediaSingle", "content": [{"type": "media", "attrs": {"id": "m1", "alt": "chart.png"}}]}
    ]));
    let text = md(doc);
    assert!(text.contains("## Plan"));
    assert!(text.contains("[docs](https://x.example)"));
    assert!(text.contains("**now**"));
    assert!(text.contains("@Sam Lee"));
    assert!(!text.contains("@@"));
    assert!(text.contains("[In Progress]"));
    assert!(text.contains("2026-01-10"));
    assert!(text.contains("<https://x.example/c>"));
    assert!(text.contains("- a"));
    assert!(text.contains("2. b"));
    assert!(text.contains("---"));
    assert!(text.contains("> q") && text.contains("> p"));
    assert!(text.contains("<https://x.example/b>"));
    assert!(text.contains("[attachment: chart.png]"));
}

#[test]
fn a_table_becomes_a_markdown_table_of_cell_text() {
    let cell = |kind: &str, t: &str| json!({"type": kind, "content": [{"type": "paragraph", "content": [{"type": "text", "text": t}]}]});
    let doc = doc_of(json!([{"type": "table", "content": [
        {"type": "tableRow", "content": [cell("tableHeader", "Name"), cell("tableHeader", "Qty")]},
        {"type": "tableRow", "content": [cell("tableCell", "a|b"), cell("tableCell", "2")]}
    ]}]));
    let text = md(doc);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], "| Name | Qty |");
    assert_eq!(lines[1], "| --- | --- |");
    assert_eq!(lines[2], "| a\\|b | 2 |");
}

#[test]
fn an_unknown_node_is_its_text_or_the_placeholder() {
    let with_text = doc_of(json!([{"type": "expand", "content": [
        {"type": "paragraph", "content": [{"type": "text", "text": "inside"}]}]}]));
    assert_eq!(md(with_text), "inside");
    let without = doc_of(json!([{"type": "extension", "attrs": {"extensionKey": "x"}}]));
    assert_eq!(md(without), "[Jira content ordin can't show: extension]");
}

#[test]
fn a_value_that_is_not_a_doc_is_response_invalid() {
    for bad in [json!({"type": "paragraph"}), json!("text"), json!(null)] {
        assert!(matches!(
            JiraAdapter::adf_to_markdown(&bad),
            Err(JiraError::ResponseInvalid)
        ));
    }
}

#[test]
fn a_code_block_with_backticks_gets_a_longer_fence() {
    let doc = doc_of(json!([{"type": "codeBlock", "attrs": {"language": "md"},
        "content": [{"type": "text", "text": "```\ninner\n```"}]}]));
    let (text, blocks) = JiraAdapter::adf_to_markdown(&doc).unwrap();
    assert!(text.starts_with("````md\n"));
    assert_eq!(blocks[0].text, "```\ninner\n```");
}

#[test]
fn a_deeply_nested_document_is_read_without_overflowing() {
    let mut node = json!({"type": "text", "text": "deep"});
    for _ in 0..200 {
        node = json!({"type": "blockquote", "content": [node]});
    }
    assert!(JiraAdapter::adf_to_markdown(&doc_of(json!([node]))).is_ok());
}

#[test]
fn quoting_escapes_backslash_and_quote_and_drops_control_characters() {
    assert_eq!(quote("a\"b\\c\u{0}d\u{7f}e\nf"), "\"a\\\"b\\\\cdef\"");
}

#[test]
fn text_that_is_an_issue_key_is_a_key_lookup() {
    let built = build(&JiraQuery::Text(" abc-12 ".into()), 10).unwrap();
    assert_eq!(built, Built::Key("ABC-12".into()));
}

#[test]
fn query_bounds_are_enforced() {
    let handovers = |label: &str, keys: Vec<String>| JiraQuery::HandoversFor {
        label: label.into(),
        also_keys: keys,
    };
    let bad = [
        (handovers("Upper", vec![]), 10),
        (handovers("", vec![]), 10),
        (handovers(&"a".repeat(65), vec![]), 10),
        (
            handovers("ok", (1..=51).map(|n| format!("ABC-{n}")).collect()),
            10,
        ),
        (handovers("ok", vec![]), 0),
        (handovers("ok", vec![]), 51),
        (JiraQuery::Text("   ".into()), 10),
        (JiraQuery::Text("x".repeat(201)), 10),
        (JiraQuery::Text("x".into()), 26),
        (JiraQuery::Text("x".into()), 0),
    ];
    for (query, limit) in bad {
        assert_eq!(
            build(&query, limit),
            Err(JiraError::QueryInvalid),
            "{query:?} {limit}"
        );
    }
    let ok = build(&handovers("ordin-handover", vec![]), 50).unwrap();
    let Built::Jql { jql, max_results } = ok else {
        panic!("a JQL search");
    };
    assert_eq!(max_results, 50);
    assert_eq!(
        jql,
        "labels = \"ordin-handover\" AND ((assignee = currentUser() AND statusCategory != Done)) ORDER BY updated DESC"
    );
}

#[test]
fn addresses_follow_the_forms_and_the_rules() {
    let project = |a: &str| JiraAdapter::parse_project_address(a);
    let issue = |a: &str| JiraAdapter::parse_issue_address(a);
    for (address, key) in [
        ("https://acme.atlassian.net/projects/ABC", "ABC"),
        (
            "https://ACME.atlassian.net/jira/software/projects/abc/boards/3",
            "ABC",
        ),
        ("https://acme.atlassian.net/jira/core/projects/ABC", "ABC"),
        (
            "https://acme.atlassian.net/jira/software/c/projects/ABC/list",
            "ABC",
        ),
        ("https://acme.atlassian.net/browse/ABC", "ABC"),
    ] {
        let parsed = project(address).unwrap_or_else(|e| panic!("{address}: {e}"));
        assert_eq!(
            (parsed.site.as_str(), parsed.key.as_str()),
            ("acme.atlassian.net", key)
        );
    }
    assert_eq!(
        issue("https://acme.atlassian.net/browse/abc-12/extra?x=1")
            .unwrap()
            .key,
        "ABC-12"
    );
    assert_eq!(
        issue("https://acme.atlassian.net/jira/core/projects/ABC?selectedIssue=ABC-7")
            .unwrap()
            .key,
        "ABC-7"
    );
    let too_long = format!(
        "https://acme.atlassian.net/browse/ABC-1?x={}",
        "a".repeat(2100)
    );
    for bad in [
        "https://acme.atlassian.net:8443/browse/ABC-1",
        "https://user@acme.atlassian.net/browse/ABC-1",
        "https://acme.atlassian.net/browse/ABC-12345678901",
        "https://acme.atlassian.net/browse/A-1",
        "https://acme.atlassian.net/jira/software/projects/ABC?selectedIssue=bad",
        "https://.atlassian.net/browse/ABC-1",
        "https://evilatlassian.net/browse/ABC-1",
        "ftp://acme.atlassian.net/browse/ABC-1",
        "not a url",
        too_long.as_str(),
    ] {
        assert_eq!(issue(bad), Err(JiraError::AddressInvalid), "{bad}");
    }
    assert_eq!(
        project("https://acme.atlassian.net/browse/ABC-12"),
        Err(JiraError::AddressInvalid)
    );
    assert_eq!(
        project("https://acme.atlassian.net/"),
        Err(JiraError::AddressInvalid)
    );
}

#[test]
fn canonical_addresses_round_trip() {
    let p = JiraAdapter::parse_project_address("https://acme.atlassian.net/projects/ABC").unwrap();
    assert_eq!(
        JiraAdapter::project_address(&p),
        "https://acme.atlassian.net/projects/ABC"
    );
    let i = JiraAdapter::parse_issue_address("https://acme.atlassian.net/browse/ABC-12").unwrap();
    assert_eq!(
        JiraAdapter::issue_address(&i),
        "https://acme.atlassian.net/browse/ABC-12"
    );
}
