// Copyright 2026 Salesforce, Inc. All rights reserved.

//! Streamable HTTP / SSE: each `data:` JSON-RPC frame is rewritten
//! independently and re-emitted as valid SSE.

mod common;

use common::ConfigurableBackend;
use mcp_apps_policy::*;
use pdk_unit::{UnitHttpMessage, UnitHttpRequest, UnitTestBuilder};
use serde_json::json;

#[test]
fn sse_tools_list_frame_is_annotated_inline() {
    let (backend, handle) = ConfigurableBackend::new();
    let frame_json = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": {
            "tools": [{"name": "get_inventory"}]
        }
    });
    let body = format!("event: message\ndata: {}\n\n", frame_json);
    handle.set_response(200, "text/event-stream", body.into_bytes());

    let mut tester = UnitTestBuilder::default()
        .with_config("{}".to_string())
        .with_backend(backend)
        .with_entrypoint(configure);

    let req = UnitHttpRequest::post()
        .with_path("/mcp")
        .with_header("content-type", "application/json")
        .with_body(b"{}".to_vec());
    let resp = tester.request(req);
    let out = String::from_utf8(resp.body().to_vec()).expect("utf8 body");

    // The wrapper survives.
    assert!(out.starts_with("event: message\n"));
    assert!(out.ends_with("\n\n"));

    // Extract the data: line and verify the policy injected
    // _meta.ui.resourceUri inside the JSON-RPC envelope.
    let data_line = out
        .lines()
        .find_map(|l| l.strip_prefix("data: "))
        .expect("data: frame present");
    let parsed: serde_json::Value = serde_json::from_str(data_line).expect("data is JSON");
    assert_eq!(
        parsed["result"]["tools"][0]["_meta"]["ui"]["resourceUri"],
        "ui://mcp-apps-policy/get_inventory"
    );
}

#[test]
fn sse_heartbeat_frames_are_passed_through() {
    let (backend, handle) = ConfigurableBackend::new();
    let body = ": ping\n\nevent: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"tools\":[]}}\n\n";
    handle.set_response(200, "text/event-stream", body.as_bytes().to_vec());

    let mut tester = UnitTestBuilder::default()
        .with_config("{}".to_string())
        .with_backend(backend)
        .with_entrypoint(configure);

    let req = UnitHttpRequest::post()
        .with_path("/mcp")
        .with_header("content-type", "application/json")
        .with_body(b"{}".to_vec());
    let resp = tester.request(req);
    let out = String::from_utf8(resp.body().to_vec()).expect("utf8 body");

    assert!(
        out.contains(": ping"),
        "heartbeat comment frame must be preserved verbatim, got: {out}"
    );
}
