//! MCP Apps transformation policy entrypoint.
//!
//! Two-phase pipeline, MCP-only:
//!
//! 1. `on_request` reads the JSON-RPC body. If it's a
//!    `resources/read` for a `ui://mcp-apps-policy/<tool>` URI, the
//!    request is short-circuited with the embedded HTML bundle so the
//!    upstream MCP server never sees it. Otherwise the request flows
//!    through unchanged.
//!
//! 2. `on_response` parses the upstream JSON-RPC response (plain JSON
//!    or SSE-wrapped) and rewrites `tools/list`, `resources/list`, and
//!    `tools/call` results in place per the configured master switches.
//!
//! Non-JSON-RPC traffic is passed through untouched. Hosts that don't
//! know about the MCP Apps extension simply ignore the `_meta.ui`
//! fields the policy injects.

mod bundle;
mod config;
mod generated;
mod mcp;

use std::rc::Rc;

use anyhow::anyhow;
use pdk::cache::CacheBuilder;
use pdk::hl::*;
use pdk::logger;
use serde_json::{json, Value};

use crate::config::PolicyConfig;
use crate::generated::config::Config;
use crate::mcp::sse;

const MCP_APP_MIME: &str = "text/html;profile=mcp-app";

/// Per-request handoff to the response phase.
#[derive(Clone, Debug, Default)]
struct RequestState {
    /// Path captured at request time so the response phase has it for
    /// debug headers.
    path: String,
}

#[entrypoint]
pub async fn configure(
    launcher: Launcher,
    Configuration(bytes): Configuration,
    _cache_builder: CacheBuilder,
) -> anyhow::Result<()> {
    let raw: Config = serde_json::from_slice(&bytes)
        .map_err(|e| anyhow!("invalid policy configuration: {e}"))?;

    let cfg = PolicyConfig::from_raw(&raw)
        .map_err(|e| anyhow!("policy configuration rejected: {e}"))?;

    logger::info!(
        "mcp-apps-policy: loaded appifyTools={} appifyResponses={} appifyActions={} renderer={} tools={} bundles={} deny={}",
        cfg.appify_tools,
        cfg.appify_responses,
        cfg.appify_actions,
        cfg.renderer.as_str(),
        cfg.tools.len(),
        cfg.custom_bundles.len(),
        cfg.deny_tools.len()
    );

    let cfg = Rc::new(cfg);
    let request_cfg = cfg.clone();
    let response_cfg = cfg;

    let filter = on_request(move |request, _client: HttpClient| {
        let cfg = request_cfg.clone();
        async move { request_filter(request, cfg).await }
    })
    .on_response(
        move |response, _client: HttpClient, data: RequestData<RequestState>| {
            let cfg = response_cfg.clone();
            async move { response_filter(response, cfg, data).await }
        },
    );

    launcher.launch(filter).await?;
    Ok(())
}

async fn request_filter(
    request: RequestHeadersState,
    cfg: Rc<PolicyConfig>,
) -> Flow<RequestState> {
    let path_with_qs = request.path();
    let path_only = path_with_qs
        .split_once('?')
        .map(|(p, _)| p.to_string())
        .unwrap_or_else(|| path_with_qs.clone());

    if !request.contains_body() {
        return Flow::Continue(RequestState { path: path_only });
    }

    // Read the body to peek at the JSON-RPC envelope. If it's a
    // resources/read for one of our ui:// URIs, we serve the HTML
    // bundle ourselves and never forward the request upstream.
    let body_state = request.into_body_state().await;
    let body = body_state.handler().body();

    if body.len() > cfg.max_body_bytes {
        // Don't try to parse oversized bodies; just let them through.
        return Flow::Continue(RequestState { path: path_only });
    }

    let parsed: Option<Value> = serde_json::from_slice(&body).ok();

    if let Some(intercept) = parsed.as_ref().and_then(|v| match_resources_read(v)) {
        if let Some(tool_name) = mcp::parse_ui_uri(&intercept.uri) {
            // Build the JSON-RPC reply locally.
            let (html, ui_meta) = bundle::html_for(&cfg, &tool_name);
            let mut contents = json!({
                "uri": intercept.uri,
                "mimeType": MCP_APP_MIME,
                "text": html,
            });
            if !ui_meta.as_object().map(|m| m.is_empty()).unwrap_or(true) {
                if let Some(map) = contents.as_object_mut() {
                    map.insert(
                        "_meta".to_string(),
                        json!({ "ui": ui_meta }),
                    );
                }
            }
            let reply = json!({
                "jsonrpc": "2.0",
                "id": intercept.id,
                "result": { "contents": [contents] }
            });
            let body_bytes = serde_json::to_vec(&reply).unwrap_or_default();
            let response = Response::new(200)
                .with_headers(vec![
                    ("content-type".into(), "application/json".into()),
                    ("x-mcp-apps-served".into(), "ui-bundle".into()),
                    ("x-mcp-apps-tool".into(), tool_name.clone()),
                ])
                .with_body(body_bytes);
            logger::info!(
                "mcp-apps-policy: served embedded UI bundle for tool '{tool_name}' (uri={})",
                intercept.uri
            );
            return Flow::Break(response);
        }
    }

    Flow::Continue(RequestState { path: path_only })
}

/// What `match_resources_read` returns when the request is a hit.
struct ResourcesRead {
    id: Value,
    uri: String,
}

/// Returns `Some(...)` when the parsed body is a JSON-RPC
/// `resources/read` request whose URI is one of ours.
fn match_resources_read(v: &Value) -> Option<ResourcesRead> {
    let map = v.as_object()?;
    if map.get("jsonrpc").and_then(|x| x.as_str()) != Some("2.0") {
        return None;
    }
    if map.get("method").and_then(|x| x.as_str()) != Some("resources/read") {
        return None;
    }
    let id = map.get("id").cloned().unwrap_or(Value::Null);
    let uri = map
        .get("params")
        .and_then(|p| p.get("uri"))
        .and_then(|v| v.as_str())?
        .to_string();
    if !uri.starts_with(&format!("ui://{}/", mcp::UI_AUTHORITY)) {
        return None;
    }
    Some(ResourcesRead { id, uri })
}

async fn response_filter(
    response: ResponseHeadersState,
    cfg: Rc<PolicyConfig>,
    data: RequestData<RequestState>,
) {
    let state = match data {
        RequestData::Continue(s) => s,
        _ => return,
    };
    if !response.contains_body() {
        return;
    }
    let original_content_type = response.handler().header("content-type");
    let is_sse_response = sse::is_sse(original_content_type.as_deref());
    let is_json_response = is_jsonish(original_content_type.as_deref());
    if !is_sse_response && !is_json_response {
        return;
    }

    // Drop content-length so the gateway recomputes it whether or not
    // we end up rewriting the body.
    response.handler().remove_header("content-length");

    let body_state = response.into_body_state().await;
    let body = body_state.handler().body();

    if body.len() > cfg.max_body_bytes {
        logger::warn!(
            "mcp-apps-policy: response body ({} bytes) exceeds maxBodyBytes ({}); passing through",
            body.len(),
            cfg.max_body_bytes
        );
        return;
    }

    let final_body: Option<Vec<u8>> = if is_sse_response {
        transform_sse_body(&cfg, &state, &body)
    } else {
        transform_json_body(&cfg, &state, &body)
    };

    if let Some(bytes) = final_body {
        if let Err(e) = body_state.handler().set_body(&bytes) {
            logger::error!("mcp-apps-policy: set_body failed: {e:?}");
        }
    }
}

/// Plain JSON path: parse, rewrite, re-serialise. Returns `None` to
/// indicate "leave the body as-is".
fn transform_json_body(
    cfg: &PolicyConfig,
    state: &RequestState,
    body: &[u8],
) -> Option<Vec<u8>> {
    let mut parsed: Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(e) => {
            logger::debug!("mcp-apps-policy: response is not JSON ({e}); passing through");
            return None;
        }
    };
    let action = mcp::rewrite_response(cfg, &mut parsed);
    if action == mcp::Action_::Untouched {
        return None;
    }
    log_action(state, action);
    serde_json::to_vec(&parsed).ok()
}

/// SSE path: walk frames, rewrite each `data:` JSON envelope.
fn transform_sse_body(
    cfg: &PolicyConfig,
    state: &RequestState,
    body: &[u8],
) -> Option<Vec<u8>> {
    let body_str = std::str::from_utf8(body).ok()?;
    let events = sse::parse(body_str);
    if events.is_empty() {
        return None;
    }
    let mut out = String::new();
    let mut transformed_any = false;
    for event in &events {
        let next = match sse::data_as_json(event) {
            Some(mut parsed) => {
                let action = mcp::rewrite_response(cfg, &mut parsed);
                if action != mcp::Action_::Untouched {
                    transformed_any = true;
                    log_action(state, action);
                    sse::render_event(event, &parsed)
                } else {
                    sse::render_event_passthrough(event)
                }
            }
            None => sse::render_event_passthrough(event),
        };
        out.push_str(&next);
    }
    if transformed_any {
        Some(out.into_bytes())
    } else {
        None
    }
}

fn log_action(state: &RequestState, action: mcp::Action_) {
    logger::debug!(
        "mcp-apps-policy: path={} action={}",
        state.path,
        action.label()
    );
}

fn is_jsonish(content_type: Option<&str>) -> bool {
    let Some(ct) = content_type else { return false };
    let lc = ct.to_ascii_lowercase();
    lc.starts_with("application/json")
        || lc.starts_with("application/problem+json")
        || lc.contains("+json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jsonish_detection() {
        assert!(is_jsonish(Some("application/json")));
        assert!(is_jsonish(Some("application/json; charset=utf-8")));
        assert!(!is_jsonish(Some("text/html")));
        assert!(!is_jsonish(None));
    }

    #[test]
    fn matches_resources_read_for_our_uri() {
        let body = json!({
            "jsonrpc": "2.0",
            "id": 7,
            "method": "resources/read",
            "params": {"uri": "ui://mcp-apps-policy/get_inventory"}
        });
        let m = match_resources_read(&body).unwrap();
        assert_eq!(m.id, json!(7));
        assert_eq!(m.uri, "ui://mcp-apps-policy/get_inventory");
    }

    #[test]
    fn ignores_resources_read_for_other_uris() {
        let body = json!({
            "jsonrpc": "2.0",
            "id": 7,
            "method": "resources/read",
            "params": {"uri": "ui://upstream-server/their-app"}
        });
        assert!(match_resources_read(&body).is_none());
    }

    #[test]
    fn ignores_non_resources_read_calls() {
        let body = json!({
            "jsonrpc": "2.0",
            "id": 7,
            "method": "tools/call",
            "params": {"name": "get_inventory"}
        });
        assert!(match_resources_read(&body).is_none());
    }
}

// Re-exports used by integration tests in the `tests/` directory.
pub use config::PolicyConfig as _PolicyConfig;
pub use generated::config::Config as _RawConfig;
pub use mcp::{rewrite_response as _rewrite_response, synthesize_ui_uri as _synthesize_ui_uri, Action_ as _Action};
pub use bundle::AUTO_HTML as _AUTO_HTML;
