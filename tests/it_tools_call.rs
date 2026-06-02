// Copyright 2026 Salesforce, Inc. All rights reserved.

//! `tools/call` shaping: structuredContent is filled in from a
//! JSON-bearing text content block, and `_meta.ui.actions` is
//! injected when the policy has actions configured for the tool.

mod common;

use common::{parse_json, ConfigurableBackend};
use mcp_apps_policy::*;
use pdk_unit::{UnitHttpMessage, UnitHttpRequest, UnitTestBuilder};
use serde_json::json;

#[test]
fn json_text_content_is_promoted_to_structured_content() {
    let (backend, handle) = ConfigurableBackend::new();
    handle.set_json(&json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": {
            "content": [
                {"type": "text", "text": "{\"sku\":\"A1\",\"qty\":5}"}
            ]
        }
    }));

    let mut tester = UnitTestBuilder::default()
        .with_config("{}".to_string())
        .with_backend(backend)
        .with_entrypoint(configure);

    let req = UnitHttpRequest::post()
        .with_path("/mcp")
        .with_header("content-type", "application/json")
        .with_body(b"{}".to_vec());
    let resp = tester.request(req);

    let body = parse_json(resp.body());
    assert_eq!(body["result"]["structuredContent"]["sku"], "A1");
    assert_eq!(body["result"]["structuredContent"]["qty"], 5);
    // The original `content` block must remain so the model still has
    // something to read.
    assert!(body["result"]["content"][0]["text"].is_string());
}

#[test]
fn injects_action_buttons_when_tool_name_is_known() {
    let (backend, handle) = ConfigurableBackend::new();
    handle.set_json(&json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": {
            "content": [{"type": "text", "text": "{\"sku\":\"A1\"}"}],
            "_meta": {"toolName": "get_inventory"}
        }
    }));

    let cfg = json!({
        "tools": [{
            "name": "get_inventory",
            "actions": [{
                "tool": "create_order",
                "label": "Order",
                "argsTemplate": "{\"sku\":\"${sku}\"}"
            }]
        }]
    });

    let mut tester = UnitTestBuilder::default()
        .with_config(cfg.to_string())
        .with_backend(backend)
        .with_entrypoint(configure);

    let req = UnitHttpRequest::post()
        .with_path("/mcp")
        .with_header("content-type", "application/json")
        .with_body(b"{}".to_vec());
    let resp = tester.request(req);

    let body = parse_json(resp.body());
    let actions = body["result"]["_meta"]["ui"]["actions"]
        .as_array()
        .expect("actions array");
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0]["tool"], "create_order");
    assert_eq!(actions[0]["label"], "Order");
    assert_eq!(actions[0]["arguments"]["sku"], "A1");
    let uri = body["result"]["_meta"]["ui"]["resourceUri"]
        .as_str()
        .unwrap();
    assert_eq!(
        uri,
        format!(
            "ui://mcp-apps-policy/v{}/get_inventory",
            env!("CARGO_PKG_VERSION")
        )
    );

    // Spec-namespaced key must mirror the alias for SEP-1865 hosts.
    let spec_key = &body["result"]["_meta"]["io.modelcontextprotocol/ui"];
    assert_eq!(spec_key, &body["result"]["_meta"]["ui"]);
    assert_eq!(spec_key["resourceUri"], uri);
}

#[test]
fn appify_responses_off_leaves_result_alone() {
    let (backend, handle) = ConfigurableBackend::new();
    handle.set_json(&json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": {
            "content": [{"type": "text", "text": "{\"sku\":\"A1\"}"}]
        }
    }));

    let mut tester = UnitTestBuilder::default()
        .with_config(
            json!({
                "appifyResponses": false,
                "appifyActions": false
            })
            .to_string(),
        )
        .with_backend(backend)
        .with_entrypoint(configure);

    let req = UnitHttpRequest::post()
        .with_path("/mcp")
        .with_header("content-type", "application/json")
        .with_body(b"{}".to_vec());
    let resp = tester.request(req);

    let body = parse_json(resp.body());
    assert!(
        body["result"].get("structuredContent").is_none(),
        "with appifyResponses=false the policy must not promote content into structuredContent"
    );
    assert!(
        body["result"].get("_meta").is_none(),
        "with appifyActions=false the policy must not inject _meta.ui.actions"
    );
}
