// Copyright 2026 Salesforce, Inc. All rights reserved.

//! `resources/read` short-circuit: requests for a
//! `ui://mcp-apps-policy/<tool>` URI are answered locally with the
//! embedded HTML bundle. The upstream MCP server is never called.

mod common;

use common::{parse_json, ConfigurableBackend};
use mcp_apps_policy::*;
use pdk_unit::{UnitHttpMessage, UnitHttpRequest, UnitTestBuilder};
use serde_json::json;

#[test]
fn ui_uri_is_served_locally_with_html_bundle() {
    let (backend, handle) = ConfigurableBackend::new();
    // Set the upstream to something easy to detect, in case the policy
    // accidentally forwards the request.
    handle.set_json(&json!({"jsonrpc": "2.0", "id": 1, "result": {"contents": []}}));

    let mut tester = UnitTestBuilder::default()
        .with_config("{}".to_string())
        .with_backend(backend)
        .with_entrypoint(configure);

    let req_body = json!({
        "jsonrpc": "2.0",
        "id": 99,
        "method": "resources/read",
        "params": {"uri": "ui://mcp-apps-policy/get_inventory"}
    });
    let req = UnitHttpRequest::post()
        .with_path("/mcp")
        .with_header("content-type", "application/json")
        .with_body(req_body.to_string().into_bytes());
    let resp = tester.request(req);

    assert_eq!(resp.status_code(), 200);
    assert!(!handle.was_called(), "upstream must not be called for ui:// URIs");

    let body = parse_json(resp.body());
    assert_eq!(body["jsonrpc"], "2.0");
    assert_eq!(body["id"], 99);
    let contents = body["result"]["contents"].as_array().unwrap();
    assert_eq!(contents.len(), 1);
    assert_eq!(
        contents[0]["uri"].as_str().unwrap(),
        "ui://mcp-apps-policy/get_inventory"
    );
    assert_eq!(
        contents[0]["mimeType"].as_str().unwrap(),
        "text/html;profile=mcp-app"
    );
    let text = contents[0]["text"].as_str().unwrap();
    assert!(text.starts_with("<!DOCTYPE html>"));
    assert!(text.contains("ui/initialize"));
    assert!(text.contains("ui/notifications/tool-result"));
}

#[test]
fn other_resources_read_calls_pass_through() {
    let (backend, handle) = ConfigurableBackend::new();
    handle.set_json(&json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": {
            "contents": [
                {"uri": "file:///etc/hosts", "mimeType": "text/plain", "text": "..."}
            ]
        }
    }));

    let mut tester = UnitTestBuilder::default()
        .with_config("{}".to_string())
        .with_backend(backend)
        .with_entrypoint(configure);

    let req_body = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "resources/read",
        "params": {"uri": "file:///etc/hosts"}
    });
    let req = UnitHttpRequest::post()
        .with_path("/mcp")
        .with_header("content-type", "application/json")
        .with_body(req_body.to_string().into_bytes());
    let resp = tester.request(req);

    assert!(handle.was_called(), "upstream must handle non-ui:// reads");
    let body = parse_json(resp.body());
    assert_eq!(
        body["result"]["contents"][0]["uri"],
        "file:///etc/hosts"
    );
}
