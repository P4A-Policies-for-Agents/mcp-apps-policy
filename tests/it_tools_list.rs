// Copyright 2026 Salesforce, Inc. All rights reserved.

//! `tools/list` annotation: every advertised tool gets
//! `_meta.ui.resourceUri` so MCP Apps hosts know it can be rendered.

mod common;

use common::{parse_json, ConfigurableBackend};
use mcp_apps_policy::*;
use pdk_unit::{UnitHttpMessage, UnitHttpRequest, UnitTestBuilder};
use serde_json::json;

fn default_config() -> serde_json::Value {
    json!({
        "appifyTools": true,
        "appifyResponses": true,
        "appifyActions": true
    })
}

#[test]
fn each_tool_gets_a_ui_resource_uri() {
    let (backend, handle) = ConfigurableBackend::new();
    handle.set_json(&json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": {
            "tools": [
                {"name": "get_inventory", "description": "Look up stock"},
                {"name": "list_customers", "description": "List customers"}
            ]
        }
    }));

    let mut tester = UnitTestBuilder::default()
        .with_config(default_config().to_string())
        .with_backend(backend)
        .with_entrypoint(configure);

    let req = UnitHttpRequest::post()
        .with_path("/mcp")
        .with_header("content-type", "application/json")
        .with_body(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\"}".to_vec());
    let resp = tester.request(req);

    let body = parse_json(resp.body());
    let tools = body["result"]["tools"].as_array().expect("tools array");
    assert_eq!(tools.len(), 2);
    let prefix = format!("ui://mcp-apps-policy/v{}/", env!("CARGO_PKG_VERSION"));
    assert_eq!(
        tools[0]["_meta"]["ui"]["resourceUri"].as_str().unwrap(),
        format!("{prefix}get_inventory")
    );
    assert_eq!(
        tools[1]["_meta"]["ui"]["resourceUri"].as_str().unwrap(),
        format!("{prefix}list_customers")
    );
    assert_eq!(tools[0]["name"], "get_inventory");
    assert_eq!(tools[0]["description"], "Look up stock");

    // Strict SEP-1865 hosts (Claude.ai) read the spec-namespaced
    // `_meta["io.modelcontextprotocol/ui"]` key and reject responses
    // that only carry the relaxed `_meta.ui` alias. Both must be set.
    let spec_key = &tools[0]["_meta"]["io.modelcontextprotocol/ui"];
    assert_eq!(
        spec_key["resourceUri"].as_str().unwrap(),
        format!("{prefix}get_inventory")
    );
    assert_eq!(spec_key, &tools[0]["_meta"]["ui"]);
}

#[test]
fn deny_listed_tools_are_left_alone() {
    let (backend, handle) = ConfigurableBackend::new();
    handle.set_json(&json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": {
            "tools": [
                {"name": "secret_admin"},
                {"name": "public_tool"}
            ]
        }
    }));

    let mut tester = UnitTestBuilder::default()
        .with_config(
            json!({
                "denyTools": ["secret_admin"]
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
    let tools = body["result"]["tools"].as_array().unwrap();
    assert!(tools[0].get("_meta").is_none(), "denied tool must not be appified");
    assert!(tools[1]["_meta"]["ui"]["resourceUri"].is_string());
}

#[test]
fn appify_tools_off_disables_advertisement() {
    let (backend, handle) = ConfigurableBackend::new();
    handle.set_json(&json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": {
            "tools": [{"name": "get_inventory"}]
        }
    }));

    let mut tester = UnitTestBuilder::default()
        .with_config(
            json!({
                "appifyTools": false,
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
    let tools = body["result"]["tools"].as_array().unwrap();
    assert!(
        tools[0].get("_meta").is_none(),
        "with appifyTools=false the policy must not inject _meta.ui"
    );
}
