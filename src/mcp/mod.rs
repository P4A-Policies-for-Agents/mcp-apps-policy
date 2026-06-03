//! MCP-aware message classification and rewriting.
//!
//! The policy only acts on JSON-RPC 2.0 envelopes (`{"jsonrpc":"2.0",
//! "id":..., "result":...}` or matching `error`). Anything else is
//! treated as opaque and passed through.
//!
//! Three response paths are implemented (gated by config):
//!
//! 1. `tools/list`     — annotate each entry's `_meta.ui.resourceUri`.
//! 2. `resources/list` — append `ui://` entries for every advertised
//!                       tool.
//! 3. `tools/call`     — copy parsed JSON content into
//!                       `structuredContent`, optionally inject
//!                       `_meta.ui.actions`.
//!
//! Method classification is best-effort: JSON-RPC responses don't
//! carry the method name, but in practice MCP clients use distinct
//! request `id`s per outstanding call. The policy looks at the *shape*
//! of `result` (presence of `tools[]`, `resources[]`, `content[]`,
//! `structuredContent`) to decide which transform applies. This is
//! the same shape-based heuristic Claude Desktop / VS Code MCP
//! clients rely on internally.

use serde_json::{json, Value};

use crate::config::{Action, ActionMode, PolicyConfig, RendererRef, SelectMode};

pub mod sse;

/// Sentinel host for the `ui://` URIs the policy synthesises. The
/// authority disambiguates from any `ui://` URIs the upstream MCP
/// server might have already served. Tools whose name contains
/// characters illegal in a URI authority/path are URL-encoded.
pub const UI_AUTHORITY: &str = "mcp-apps-policy";

/// Spec-namespaced `_meta` key for the MCP Apps extension (SEP-1865).
/// Claude.ai's host requires this exact key — `_meta.ui` is rejected
/// with "no ui/resourceUri in tool._meta". We write under both keys so
/// strict (Claude.ai) and relaxed (Inspector / our own bundle) hosts
/// both find the metadata.
pub const UI_META_KEY: &str = "io.modelcontextprotocol/ui";

/// Version segment baked into every minted `ui://` URI. Hosts (notably
/// Claude.ai's `*.claudemcpcontent.com` sandbox proxy) cache the
/// bundle keyed by URI; when we ship a new bundle we want the URI to
/// change so the cache misses. Tying the segment to `CARGO_PKG_VERSION`
/// gets that for free on every release.
pub const POLICY_VERSION: &str = env!("CARGO_PKG_VERSION");

/// What the policy decided to do with one JSON-RPC frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action_ {
    Untouched,
    AnnotatedToolsList,
    AppendedUiResources,
    NormalisedToolResult,
    InjectedActions,
    NormalisedAndInjected,
}

impl Action_ {
    pub fn label(self) -> &'static str {
        match self {
            Self::Untouched => "untouched",
            Self::AnnotatedToolsList => "annotated-tools-list",
            Self::AppendedUiResources => "appended-ui-resources",
            Self::NormalisedToolResult => "normalised-tool-result",
            Self::InjectedActions => "injected-actions",
            Self::NormalisedAndInjected => "normalised+injected",
        }
    }
}

/// Inspect a JSON-RPC response and apply the configured transforms.
/// Returns `(maybe_new_value, action)`. When `action == Untouched` the
/// caller can keep the original frame to avoid re-serialising.
///
/// `request_tool_name` is the tool name pulled from the originating
/// `tools/call` request (`params.name`), if any. JSON-RPC responses
/// don't carry the method name and most upstream MCP servers don't
/// emit `_meta.toolName`, so this is how the response phase learns
/// which tool produced the result.
pub fn rewrite_response(
    cfg: &PolicyConfig,
    body: &mut Value,
    request_tool_name: Option<&str>,
) -> Action_ {
    let Some(map) = body.as_object_mut() else {
        return Action_::Untouched;
    };
    if map.get("jsonrpc").and_then(|v| v.as_str()) != Some("2.0") {
        return Action_::Untouched;
    }
    if !map.contains_key("result") {
        return Action_::Untouched;
    }
    let Some(result) = map.get_mut("result") else {
        return Action_::Untouched;
    };

    if let Some(tools) = result.get_mut("tools").and_then(|v| v.as_array_mut()) {
        if cfg.appify_tools {
            annotate_tools(tools, cfg);
            return Action_::AnnotatedToolsList;
        }
        return Action_::Untouched;
    }

    if let Some(resources) = result.get_mut("resources").and_then(|v| v.as_array_mut()) {
        if cfg.appify_tools {
            append_ui_resources(resources, cfg);
            return Action_::AppendedUiResources;
        }
        return Action_::Untouched;
    }

    if is_tool_call_result(result) {
        return apply_to_tool_call_result(result, cfg, request_tool_name);
    }

    Action_::Untouched
}

fn is_tool_call_result(result: &Value) -> bool {
    let Some(map) = result.as_object() else {
        return false;
    };
    map.contains_key("content")
        || map.contains_key("structuredContent")
        || map.contains_key("isError")
}

/// Walk `tools[]`, attach `_meta.ui.resourceUri` to each entry not on
/// the deny-list / explicitly opted out.
fn annotate_tools(tools: &mut [Value], cfg: &PolicyConfig) {
    for tool in tools.iter_mut() {
        let name = tool
            .get("name")
            .and_then(|v| v.as_str())
            .map(String::from);
        let Some(name) = name else { continue };
        if !cfg.appifies(&name) {
            continue;
        }
        let Some(map) = tool.as_object_mut() else {
            continue;
        };
        let meta = map
            .entry("_meta".to_string())
            .or_insert_with(|| json!({}));
        let Some(meta_map) = meta.as_object_mut() else {
            continue;
        };
        // Build the UI metadata once, then mirror it under both
        // `_meta.ui` (informal alias used by Inspector / our bundle)
        // and `_meta["io.modelcontextprotocol/ui"]` (the SEP-1865
        // spec-namespaced key Claude.ai's host requires).
        // `csp` + `domain` are required by ChatGPT's app-submission
        // validator and ignored by Claude.ai (additive — safe to send
        // to both).
        let mut ui = json!({
            "resourceUri": synthesize_ui_uri(&name),
            "visibility": ["model", "app"],
            "domain": cfg.domain_for(&name),
            "csp": cfg.csp_for(&name).to_meta(),
        });
        // If the upstream already shipped MCP Apps metadata, prefer
        // its values — merge ours underneath without clobbering.
        if let Some(existing) = meta_map.get(UI_META_KEY).cloned() {
            merge_objects(&mut ui, &existing);
        } else if let Some(existing) = meta_map.get("ui").cloned() {
            merge_objects(&mut ui, &existing);
        }
        meta_map.insert(UI_META_KEY.to_string(), ui.clone());
        meta_map.insert("ui".to_string(), ui);
    }
}

/// Shallow-merge `src` into `dst` (object-only). Keys present in `src`
/// overwrite corresponding keys in `dst`. Used to honour upstream-
/// supplied UI metadata over our synthesised defaults.
fn merge_objects(dst: &mut Value, src: &Value) {
    let (Some(dst_map), Some(src_map)) = (dst.as_object_mut(), src.as_object()) else {
        return;
    };
    for (k, v) in src_map {
        dst_map.insert(k.clone(), v.clone());
    }
}

/// Append a synthesised `ui://` resource entry per appifiable tool.
/// We don't have the tool list on the wire of `resources/list`, but we
/// can opportunistically re-advertise the policy's own `ui://` resources
/// for any tool listed in the policy's per-tool overrides — that's
/// useful when admins want a specific tool's UI to appear in the host's
/// resource browser. Real MCP-Apps hosts (Claude Desktop) discover apps
/// via the `tools/list` `_meta.ui.resourceUri` field, not this list, so
/// the contents here are mostly cosmetic.
fn append_ui_resources(resources: &mut Vec<Value>, cfg: &PolicyConfig) {
    // Skip if the upstream already serves at least one ui:// resource —
    // we don't want to compete with a server that's intentionally MCP-
    // Apps-aware.
    let already_has_ui = resources.iter().any(|r| {
        r.get("uri")
            .and_then(|v| v.as_str())
            .map(|s| s.starts_with("ui://"))
            .unwrap_or(false)
    });
    if already_has_ui {
        return;
    }
    for tool_name in cfg.tools.keys() {
        if !cfg.appifies(tool_name) {
            continue;
        }
        resources.push(json!({
            "uri": synthesize_ui_uri(tool_name),
            "name": format!("{tool_name} (MCP App)"),
            "description": format!("Interactive MCP App view for the '{tool_name}' tool, served by mcp-apps-policy."),
            "mimeType": "text/html;profile=mcp-app",
        }));
    }
}

/// Apply transforms to a `tools/call` `result` object.
fn apply_to_tool_call_result(
    result: &mut Value,
    cfg: &PolicyConfig,
    request_tool_name: Option<&str>,
) -> Action_ {
    let normalised = if cfg.appify_responses {
        ensure_structured_content(result)
    } else {
        false
    };

    // Prefer the tool name captured from the originating `tools/call`
    // request — JSON-RPC responses don't carry the method name and most
    // upstream MCP servers don't emit `_meta.toolName`. Fall back to
    // `result._meta.toolName` for the rare server that does.
    let tool_name = request_tool_name.map(String::from).or_else(|| {
        result
            .get("_meta")
            .and_then(|m| m.get("toolName"))
            .and_then(|v| v.as_str())
            .map(String::from)
    });
    let actions = match tool_name.as_deref() {
        Some(name) => cfg.actions_for(name),
        None => {
            if cfg.appify_actions && cfg.deny_tools.is_empty() {
                cfg.default_actions.clone()
            } else {
                Vec::new()
            }
        }
    };

    let injected = if !actions.is_empty() {
        let structured = result
            .get("structuredContent")
            .cloned()
            .unwrap_or(Value::Null);
        let resolved: Vec<Value> = actions
            .iter()
            .map(|a| resolve_action(a, &structured))
            .collect();

        if let Some(map) = result.as_object_mut() {
            let meta = map
                .entry("_meta".to_string())
                .or_insert_with(|| json!({}));
            if let Some(meta_map) = meta.as_object_mut() {
                // Read whichever shape (spec key or alias) the result
                // already carries so we don't drop fields the upstream
                // set, then re-emit under both keys.
                let mut ui = meta_map
                    .get(UI_META_KEY)
                    .cloned()
                    .or_else(|| meta_map.get("ui").cloned())
                    .unwrap_or_else(|| json!({}));
                if let Some(ui_map) = ui.as_object_mut() {
                    ui_map.insert("actions".to_string(), Value::Array(resolved));
                    if let Some(t) = &tool_name {
                        ui_map
                            .entry("resourceUri".to_string())
                            .or_insert_with(|| Value::String(synthesize_ui_uri(t)));
                        // CSP + domain mirror the tools/list annotation
                        // path so app-aware hosts see the same template
                        // metadata on every result frame.
                        ui_map
                            .entry("domain".to_string())
                            .or_insert_with(|| Value::String(cfg.domain_for(t)));
                        ui_map
                            .entry("csp".to_string())
                            .or_insert_with(|| cfg.csp_for(t).to_meta());
                    }
                    let renderer_label = match tool_name.as_deref() {
                        Some(t) => match cfg.renderer_for(t) {
                            RendererRef::BuiltIn(r) => r.as_str().to_string(),
                            RendererRef::Custom(n) => n,
                        },
                        None => cfg.renderer.as_str().to_string(),
                    };
                    ui_map
                        .entry("renderer".to_string())
                        .or_insert_with(|| Value::String(renderer_label));
                }
                meta_map.insert(UI_META_KEY.to_string(), ui.clone());
                meta_map.insert("ui".to_string(), ui);
            }
        }
        true
    } else {
        false
    };

    match (normalised, injected) {
        (true, true) => Action_::NormalisedAndInjected,
        (true, false) => Action_::NormalisedToolResult,
        (false, true) => Action_::InjectedActions,
        (false, false) => Action_::Untouched,
    }
}

/// If `result.structuredContent` is missing and `result.content[0]` is a
/// `text` block whose body parses as JSON, copy the parsed value into
/// `result.structuredContent`. Returns true when the result was
/// modified.
fn ensure_structured_content(result: &mut Value) -> bool {
    let Some(map) = result.as_object_mut() else {
        return false;
    };
    if map.contains_key("structuredContent") {
        return false;
    }
    let Some(content) = map.get("content").and_then(|v| v.as_array()) else {
        return false;
    };
    let Some(first) = content.first() else {
        return false;
    };
    if first.get("type").and_then(|t| t.as_str()) != Some("text") {
        return false;
    }
    let Some(text) = first.get("text").and_then(|t| t.as_str()) else {
        return false;
    };
    let parsed: Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(_) => return false,
    };
    // Per the MCP spec, `structuredContent` MUST be a JSON object —
    // arrays, strings, and primitives are not valid there. Wrap a raw
    // top-level array under a stable `items` key so list-returning
    // tools (e.g. `get_accounts`) still get a usable
    // `structuredContent` object instead of being rejected by the
    // host's validator.
    let wrapped = if parsed.is_object() {
        parsed
    } else if parsed.is_array() {
        json!({ "items": parsed })
    } else {
        return false;
    };
    map.insert("structuredContent".to_string(), wrapped);
    true
}

/// Render an `_meta.ui.actions[]` entry. The shape sent to the bundle
/// depends on the action's `select` mode:
///
/// - `none`   : substitute `${field}` placeholders server-side against
///              `structuredContent` and ship resolved `arguments` (legacy
///              behaviour kept for back-compat).
/// - `single` : ship the raw template + a `select` hint so the bundle
///              renders a radio-column table and substitutes against
///              the user's selected row at click time.
/// - `multi`  : same as `single` but with a checkbox column and
///              array-projection support (`${rows}`, `${rows[].Field}`).
fn resolve_action(action: &Action, ctx: &Value) -> Value {
    let mut out = json!({
        "tool": action.tool,
        "label": action.label,
    });
    let map = out.as_object_mut().expect("just constructed object");

    // Always ship `select` and `mode` so the bundle can decide which UI
    // affordance to render. `none`/`call` are the defaults so the bundle
    // can ignore them; sending the explicit value is harmless and makes
    // the contract obvious in `_meta.ui.actions[]` payloads.
    map.insert("select".to_string(), Value::String(action.select.as_str().to_string()));
    map.insert("mode".to_string(), Value::String(action.mode.as_str().to_string()));

    match action.select {
        SelectMode::None => {
            // Legacy server-side substitution against the top-level
            // structuredContent. `${field}` not present → placeholder
            // ships through unchanged.
            if let Some(template) = &action.args_template {
                let resolved = resolve_template(template, ctx);
                map.insert("arguments".to_string(), resolved);
            }
        }
        SelectMode::Single | SelectMode::Multi => {
            // Substitution must happen against the user's row pick.
            // Ship the raw template through; the bundle will fill it
            // out client-side. We deliberately don't ship `arguments`
            // pre-filled — without a row selection there's nothing to
            // resolve against.
            if let Some(template) = &action.args_template {
                map.insert("argsTemplate".to_string(), template.clone());
            }
        }
    }
    if matches!(action.mode, ActionMode::Form) {
        // Hint the bundle to open an inline edit form built from the
        // selected row's keys, rather than firing the call straight
        // away.
        map.insert("mode".to_string(), Value::String("form".to_string()));
    }

    // `mode: "prompt"` ships a chat-message string instead of (or in
    // addition to) `arguments`. The bundle hands this to the host so
    // the agent re-decides whether to call the tool — the iframe never
    // calls the tool directly. For `select == None` we substitute
    // server-side against `structuredContent`; for `single`/`multi` the
    // template ships raw and the bundle substitutes against the row pick.
    if matches!(action.mode, ActionMode::Prompt) {
        if let Some(tmpl) = &action.prompt_template {
            match action.select {
                SelectMode::None => {
                    let resolved = substitute(tmpl, ctx);
                    map.insert("prompt".to_string(), Value::String(resolved));
                }
                SelectMode::Single | SelectMode::Multi => {
                    map.insert(
                        "promptTemplate".to_string(),
                        Value::String(tmpl.clone()),
                    );
                }
            }
        }
    }
    out
}

/// Recursively walk a JSON value and replace `${field}` placeholders
/// inside string leaves with values pulled from `ctx` by key path.
/// Only flat dotted keys (`a`, `a.b`) are supported; that covers the
/// common case (a tool result whose `structuredContent` is a flat
/// object) without inviting an expression language.
fn resolve_template(template: &Value, ctx: &Value) -> Value {
    match template {
        Value::String(s) => Value::String(substitute(s, ctx)),
        Value::Array(items) => {
            Value::Array(items.iter().map(|v| resolve_template(v, ctx)).collect())
        }
        Value::Object(map) => {
            let mut out = serde_json::Map::new();
            for (k, v) in map {
                out.insert(k.clone(), resolve_template(v, ctx));
            }
            Value::Object(out)
        }
        _ => template.clone(),
    }
}

fn substitute(input: &str, ctx: &Value) -> String {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'$' && bytes[i + 1] == b'{' {
            if let Some(end) = input[i + 2..].find('}') {
                let key = &input[i + 2..i + 2 + end];
                let value = lookup(ctx, key);
                match value {
                    Some(v) => out.push_str(&value_to_string(&v)),
                    None => out.push_str(&input[i..i + 2 + end + 1]),
                }
                i += 2 + end + 1;
                continue;
            }
        }
        out.push(input[i..].chars().next().unwrap());
        i += input[i..].chars().next().unwrap().len_utf8();
    }
    out
}

fn lookup(ctx: &Value, dotted: &str) -> Option<Value> {
    let mut cur = ctx;
    for seg in dotted.split('.') {
        cur = cur.get(seg)?;
    }
    Some(cur.clone())
}

fn value_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => "".into(),
        other => other.to_string(),
    }
}

/// Build the canonical `ui://mcp-apps-policy/v<version>/<toolName>` URI.
/// The version segment is baked in so the URI changes on every policy
/// release — that bust hosts (notably Claude.ai's sandbox proxy) which
/// cache the bundle bytes by URI.
pub fn synthesize_ui_uri(tool_name: &str) -> String {
    format!(
        "ui://{}/v{}/{}",
        UI_AUTHORITY,
        POLICY_VERSION,
        encode_segment(tool_name)
    )
}

/// Inverse of `synthesize_ui_uri`. Recognises both the current
/// versioned shape (`ui://mcp-apps-policy/v<version>/<tool>`) and the
/// pre-0.1.9 unversioned shape (`ui://mcp-apps-policy/<tool>`) so
/// stale `_meta.ui.resourceUri` references — e.g. ones cached by a
/// host between releases — still resolve to the correct tool.
/// Returns `None` if the URI was not minted by this policy.
pub fn parse_ui_uri(uri: &str) -> Option<String> {
    let prefix = format!("ui://{}/", UI_AUTHORITY);
    let rest = uri.strip_prefix(&prefix)?;
    let segment = match rest.split_once('/') {
        Some((first, tool)) if is_version_segment(first) => tool,
        _ => rest,
    };
    Some(decode_segment(segment))
}

/// Recognises a `v<digits>...` version segment without pulling in a
/// regex dep. Anything else (including a tool whose name happens to
/// start with `v`) falls through to the legacy single-segment path.
fn is_version_segment(seg: &str) -> bool {
    let Some(rest) = seg.strip_prefix('v') else {
        return false;
    };
    rest.chars()
        .next()
        .map(|c| c.is_ascii_digit())
        .unwrap_or(false)
}

fn encode_segment(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        let safe = (b'A'..=b'Z').contains(&b)
            || (b'a'..=b'z').contains(&b)
            || (b'0'..=b'9').contains(&b)
            || matches!(b, b'-' | b'_' | b'.' | b'~');
        if safe {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{:02X}", b));
        }
    }
    out
}

fn decode_segment(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (
                hex(bytes[i + 1]),
                hex(bytes[i + 2]),
            ) {
                out.push(hi * 16 + lo);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|e| {
        // Fallback to lossy decode; tool names should be ASCII-ish.
        String::from_utf8_lossy(e.as_bytes()).into_owned()
    })
}

fn hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(10 + b - b'a'),
        b'A'..=b'F' => Some(10 + b - b'A'),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generated::config::Config as RawConfig;
    use serde_json::json;

    fn cfg_default() -> PolicyConfig {
        PolicyConfig::from_raw(&RawConfig {
            appify_tools: None,
            appify_responses: None,
            appify_actions: None,
            renderer: None,
            tools: None,
            default_actions: None,
            deny_tools: None,
            form_tools: None,
            custom_bundles: None,
            csp: None,
            domain: None,
            preview_mode: None,
            debug_headers: None,
            max_body_bytes: None,
        })
        .unwrap()
    }

    #[test]
    fn ui_uri_roundtrips() {
        let uri = synthesize_ui_uri("get inventory!");
        // Versioned shape — `ui://mcp-apps-policy/v<version>/<tool>`.
        assert!(uri.starts_with(&format!(
            "ui://mcp-apps-policy/v{}/",
            env!("CARGO_PKG_VERSION")
        )));
        assert_eq!(parse_ui_uri(&uri).as_deref(), Some("get inventory!"));
    }

    #[test]
    fn ui_uri_parser_accepts_legacy_unversioned_shape() {
        // Pre-0.1.9 hosts that cached `ui://mcp-apps-policy/<tool>`
        // before we added the version segment must still resolve to the
        // right tool.
        assert_eq!(
            parse_ui_uri("ui://mcp-apps-policy/get_inventory").as_deref(),
            Some("get_inventory")
        );
    }

    #[test]
    fn ui_uri_parser_handles_tool_starting_with_v() {
        // A tool literally named `vault` (no digit after `v`) must not
        // be misread as a version segment.
        assert_eq!(
            parse_ui_uri("ui://mcp-apps-policy/vault").as_deref(),
            Some("vault")
        );
    }

    #[test]
    fn rewrite_passthrough_for_non_jsonrpc() {
        let cfg = cfg_default();
        let mut body = json!({"hello": "world"});
        assert_eq!(rewrite_response(&cfg, &mut body, None), Action_::Untouched);
        assert_eq!(body, json!({"hello": "world"}));
    }

    #[test]
    fn annotates_tools_list() {
        let cfg = cfg_default();
        let mut body = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "tools": [
                    {"name": "get_inventory", "description": "x"},
                    {"name": "list_customers", "description": "y"}
                ]
            }
        });
        assert_eq!(
            rewrite_response(&cfg, &mut body, None),
            Action_::AnnotatedToolsList
        );
        let tools = body["result"]["tools"].as_array().unwrap();
        assert_eq!(
            tools[0]["_meta"]["ui"]["resourceUri"].as_str().unwrap(),
            synthesize_ui_uri("get_inventory")
        );
        assert_eq!(
            tools[1]["_meta"]["ui"]["resourceUri"].as_str().unwrap(),
            synthesize_ui_uri("list_customers")
        );
    }

    #[test]
    fn emits_spec_namespaced_meta_key_on_tools_list() {
        // Claude.ai's host requires `_meta["io.modelcontextprotocol/ui"]`
        // — the relaxed `_meta.ui` alias is rejected with
        // "no ui/resourceUri in tool._meta". Both keys must be present.
        let cfg = cfg_default();
        let mut body = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {"tools": [{"name": "get_inventory"}]}
        });
        rewrite_response(&cfg, &mut body, None);
        let tool = &body["result"]["tools"][0];
        let spec = &tool["_meta"][UI_META_KEY];
        assert_eq!(
            spec["resourceUri"].as_str().unwrap(),
            synthesize_ui_uri("get_inventory")
        );
        assert_eq!(spec["visibility"], json!(["model", "app"]));
        // Alias must mirror it for relaxed hosts (Inspector, our bundle).
        assert_eq!(tool["_meta"]["ui"], *spec);
    }

    #[test]
    fn emits_spec_namespaced_meta_key_on_tool_call_result() {
        let cfg = PolicyConfig::from_raw(&RawConfig {
            tools: Some(vec![crate::generated::config::Tools0Config {
                name: "get_inventory".into(),
                renderer: None,
                appify: None,
                actions: Some(vec![crate::generated::config::Actions0Config {
                    tool: "create_order".into(),
                    label: "Order".into(),
                    args_template: Some("{\"sku\":\"${sku}\"}".into()),
                    select: None,
                    mode: None,
                    prompt: None,
                }]),
                csp: None,
                domain: None,
                form_fields: None,
                form_mode: None,
            }]),
            ..raw_empty()
        })
        .unwrap();
        let mut body = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "content": [{"type": "text", "text": "{\"sku\":\"A1\"}"}]
            }
        });
        rewrite_response(&cfg, &mut body, Some("get_inventory"));
        let spec = &body["result"]["_meta"][UI_META_KEY];
        let alias = &body["result"]["_meta"]["ui"];
        assert_eq!(spec, alias);
        assert_eq!(spec["actions"][0]["tool"], "create_order");
        assert_eq!(
            spec["resourceUri"].as_str().unwrap(),
            synthesize_ui_uri("get_inventory")
        );
    }

    #[test]
    fn emits_default_csp_and_synthesised_domain_on_tools_list() {
        // ChatGPT's app-submission validator requires both `_meta.ui.csp`
        // and `_meta.ui.domain` per template; missing either marks the
        // template as "Not yet submittable for review".
        let cfg = cfg_default();
        let mut body = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {"tools": [{"name": "get_inventory"}]}
        });
        rewrite_response(&cfg, &mut body, None);
        let ui = &body["result"]["tools"][0]["_meta"]["ui"];
        // Empty allowlists are valid CSP per the spec; the *presence*
        // of the block is what ChatGPT keys on.
        assert!(ui["csp"].is_object());
        assert_eq!(ui["csp"]["connectDomains"], json!([]));
        assert_eq!(ui["csp"]["resourceDomains"], json!([]));
        assert_eq!(ui["csp"]["frameDomains"], json!([]));
        assert_eq!(ui["csp"]["baseUriDomains"], json!([]));
        assert_eq!(
            ui["domain"].as_str().unwrap(),
            "get-inventory.mcp-apps-policy.local"
        );
    }

    #[test]
    fn per_tool_csp_and_domain_override_global() {
        let cfg = PolicyConfig::from_raw(&RawConfig {
            csp: Some(crate::generated::config::CspConfig {
                connect_domains: Some(vec!["api.example.com".into()]),
                resource_domains: None,
                frame_domains: None,
                base_uri_domains: None,
            }),
            domain: Some("global.example.com".into()),
            tools: Some(vec![crate::generated::config::Tools0Config {
                name: "special".into(),
                renderer: None,
                appify: None,
                actions: None,
                csp: Some(crate::generated::config::Csp1Config {
                    connect_domains: Some(vec!["special.example.com".into()]),
                    resource_domains: None,
                    frame_domains: None,
                    base_uri_domains: None,
                }),
                domain: Some("special.example.com".into()),
                form_fields: None,
                form_mode: None,
            }]),
            ..raw_empty()
        })
        .unwrap();
        let mut body = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "tools": [
                    {"name": "default_tool"},
                    {"name": "special"}
                ]
            }
        });
        rewrite_response(&cfg, &mut body, None);
        let default_ui = &body["result"]["tools"][0]["_meta"]["ui"];
        assert_eq!(
            default_ui["csp"]["connectDomains"],
            json!(["api.example.com"])
        );
        assert_eq!(default_ui["domain"], "global.example.com");
        let special_ui = &body["result"]["tools"][1]["_meta"]["ui"];
        assert_eq!(
            special_ui["csp"]["connectDomains"],
            json!(["special.example.com"])
        );
        assert_eq!(special_ui["domain"], "special.example.com");
    }

    #[test]
    fn tool_call_result_carries_csp_and_domain() {
        let cfg = cfg_default();
        let mut body = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "content": [{"type": "text", "text": "{\"sku\":\"A1\"}"}]
            }
        });
        rewrite_response(&cfg, &mut body, Some("get_inventory"));
        // No actions configured by default → no _meta.ui injected by the
        // result path; rely on tools/list for csp+domain there. Add a
        // default action so the result path runs.
        let cfg = PolicyConfig::from_raw(&RawConfig {
            default_actions: Some(vec![crate::generated::config::DefaultActions0Config {
                tool: "noop".into(),
                label: "Back".into(),
                args_template: None,
                select: None,
                mode: None,
                prompt: None,
            }]),
            ..raw_empty()
        })
        .unwrap();
        let mut body = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "content": [{"type": "text", "text": "{\"sku\":\"A1\"}"}]
            }
        });
        rewrite_response(&cfg, &mut body, Some("get_inventory"));
        let ui = &body["result"]["_meta"]["ui"];
        assert_eq!(
            ui["domain"].as_str().unwrap(),
            "get-inventory.mcp-apps-policy.local"
        );
        assert!(ui["csp"].is_object());
    }

    #[test]
    fn skips_tools_on_deny_list() {
        let cfg = PolicyConfig::from_raw(&RawConfig {
            deny_tools: Some(vec!["secret_admin".into()]),
            ..raw_empty()
        })
        .unwrap();
        let mut body = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "tools": [
                    {"name": "secret_admin", "description": "internal"},
                    {"name": "public_tool", "description": "ok"}
                ]
            }
        });
        rewrite_response(&cfg, &mut body, None);
        let tools = body["result"]["tools"].as_array().unwrap();
        assert!(tools[0].get("_meta").is_none());
        assert!(tools[1]["_meta"]["ui"]["resourceUri"].is_string());
    }

    #[test]
    fn copies_text_content_into_structured_content() {
        let cfg = cfg_default();
        let mut body = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "content": [
                    {"type": "text", "text": "{\"sku\": \"A1\", \"qty\": 5}"}
                ]
            }
        });
        let action = rewrite_response(&cfg, &mut body, None);
        assert!(matches!(
            action,
            Action_::NormalisedToolResult | Action_::NormalisedAndInjected
        ));
        assert_eq!(body["result"]["structuredContent"]["sku"], "A1");
        assert_eq!(body["result"]["structuredContent"]["qty"], 5);
    }

    #[test]
    fn wraps_top_level_arrays_under_items() {
        // MCP `structuredContent` MUST be an object. A tool whose
        // text content is a JSON array (e.g. CRM `get_accounts`) gets
        // wrapped under `{items: [...]}` so hosts don't reject it.
        let cfg = cfg_default();
        let mut body = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "content": [
                    {"type": "text", "text": "[{\"Id\":\"a\"},{\"Id\":\"b\"}]"}
                ]
            }
        });
        rewrite_response(&cfg, &mut body, None);
        assert!(body["result"]["structuredContent"].is_object());
        let items = body["result"]["structuredContent"]["items"]
            .as_array()
            .expect("items array");
        assert_eq!(items.len(), 2);
        assert_eq!(items[0]["Id"], "a");
    }

    #[test]
    fn preserves_existing_structured_content() {
        let cfg = cfg_default();
        let mut body = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "content": [{"type": "text", "text": "{\"a\":1}"}],
                "structuredContent": {"prebuilt": true}
            }
        });
        rewrite_response(&cfg, &mut body, None);
        assert_eq!(body["result"]["structuredContent"], json!({"prebuilt": true}));
    }

    #[test]
    fn injects_actions_with_template() {
        let cfg = PolicyConfig::from_raw(&RawConfig {
            tools: Some(vec![crate::generated::config::Tools0Config {
                name: "get_inventory".into(),
                renderer: None,
                appify: None,
                actions: Some(vec![crate::generated::config::Actions0Config {
                    tool: "create_order".into(),
                    label: "Order".into(),
                    args_template: Some("{\"sku\":\"${sku}\"}".into()),
                    select: None,
                    mode: None,
                    prompt: None,
                }]),
                csp: None,
                domain: None,
                form_fields: None,
                form_mode: None,
            }]),
            ..raw_empty()
        })
        .unwrap();
        let mut body = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "content": [{"type": "text", "text": "{\"sku\":\"A1\"}"}],
                "_meta": {"toolName": "get_inventory"}
            }
        });
        let action = rewrite_response(&cfg, &mut body, None);
        assert!(matches!(action, Action_::NormalisedAndInjected));
        let actions = body["result"]["_meta"]["ui"]["actions"]
            .as_array()
            .unwrap();
        assert_eq!(actions[0]["tool"], "create_order");
        assert_eq!(actions[0]["arguments"], json!({"sku": "A1"}));
    }

    #[test]
    fn injects_actions_using_request_tool_name() {
        // Real upstreams don't emit `_meta.toolName`; the request side
        // captures `params.name` and passes it down. Confirm that
        // pathway works on a result with no `_meta` at all.
        let cfg = PolicyConfig::from_raw(&RawConfig {
            tools: Some(vec![crate::generated::config::Tools0Config {
                name: "get_inventory".into(),
                renderer: None,
                appify: None,
                actions: Some(vec![crate::generated::config::Actions0Config {
                    tool: "create_order".into(),
                    label: "Order".into(),
                    args_template: Some("{\"sku\":\"${sku}\"}".into()),
                    select: None,
                    mode: None,
                    prompt: None,
                }]),
                csp: None,
                domain: None,
                form_fields: None,
                form_mode: None,
            }]),
            ..raw_empty()
        })
        .unwrap();
        let mut body = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "content": [{"type": "text", "text": "{\"sku\":\"A1\"}"}]
            }
        });
        let action = rewrite_response(&cfg, &mut body, Some("get_inventory"));
        assert!(matches!(action, Action_::NormalisedAndInjected));
        let actions = body["result"]["_meta"]["ui"]["actions"]
            .as_array()
            .unwrap();
        assert_eq!(actions[0]["tool"], "create_order");
        assert_eq!(actions[0]["arguments"], json!({"sku": "A1"}));
    }

    #[test]
    fn prompt_mode_action_select_none_substitutes_server_side() {
        // `mode: "prompt"` with no row selection: the policy
        // substitutes `${field}` against `structuredContent` and ships
        // the resolved message under `prompt`. No `arguments` field —
        // the bundle never calls the tool from inside the iframe.
        let cfg = PolicyConfig::from_raw(&RawConfig {
            tools: Some(vec![crate::generated::config::Tools0Config {
                name: "get_inventory".into(),
                renderer: None,
                appify: None,
                actions: Some(vec![crate::generated::config::Actions0Config {
                    tool: "submit_order".into(),
                    label: "Order this material".into(),
                    args_template: None,
                    select: None,
                    mode: Some("prompt".into()),
                    prompt: Some("Please order material ${productId}".into()),
                }]),
                csp: None,
                domain: None,
                form_fields: None,
                form_mode: None,
            }]),
            ..raw_empty()
        })
        .unwrap();
        let mut body = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "content": [{"type": "text", "text": "{\"productId\":\"SKU-42\"}"}]
            }
        });
        rewrite_response(&cfg, &mut body, Some("get_inventory"));
        let action = &body["result"]["_meta"]["ui"]["actions"][0];
        assert_eq!(action["mode"], "prompt");
        assert_eq!(action["prompt"], "Please order material SKU-42");
        assert!(action.get("arguments").is_none(),
            "prompt mode must not ship pre-resolved arguments");
    }

    #[test]
    fn prompt_mode_action_select_single_ships_raw_template() {
        // `mode: "prompt"` + `select: "single"`: substitution happens
        // client-side against the user's row pick, so the policy must
        // ship the template raw under `promptTemplate`.
        let cfg = PolicyConfig::from_raw(&RawConfig {
            tools: Some(vec![crate::generated::config::Tools0Config {
                name: "get_accounts".into(),
                renderer: None,
                appify: None,
                actions: Some(vec![crate::generated::config::Actions0Config {
                    tool: "delete_accounts".into(),
                    label: "Delete".into(),
                    args_template: None,
                    select: Some("single".into()),
                    mode: Some("prompt".into()),
                    prompt: Some("Delete account ${Id} (${Name})".into()),
                }]),
                csp: None,
                domain: None,
                form_fields: None,
                form_mode: None,
            }]),
            ..raw_empty()
        })
        .unwrap();
        let mut body = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "content": [{"type": "text", "text": "[]"}]
            }
        });
        rewrite_response(&cfg, &mut body, Some("get_accounts"));
        let action = &body["result"]["_meta"]["ui"]["actions"][0];
        assert_eq!(action["mode"], "prompt");
        assert_eq!(action["promptTemplate"], "Delete account ${Id} (${Name})");
        assert!(action.get("prompt").is_none(),
            "single-select prompt mode ships the raw template, not a resolved string");
    }

    #[test]
    fn appends_ui_resources_for_known_tools() {
        let cfg = PolicyConfig::from_raw(&RawConfig {
            tools: Some(vec![crate::generated::config::Tools0Config {
                name: "get_inventory".into(),
                renderer: None,
                appify: None,
                actions: None,
                csp: None,
                domain: None,
                form_fields: None,
                form_mode: None,
            }]),
            ..raw_empty()
        })
        .unwrap();
        let mut body = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {"resources": []}
        });
        let action = rewrite_response(&cfg, &mut body, None);
        assert_eq!(action, Action_::AppendedUiResources);
        let resources = body["result"]["resources"].as_array().unwrap();
        assert_eq!(resources.len(), 1);
        assert_eq!(
            resources[0]["uri"].as_str().unwrap(),
            synthesize_ui_uri("get_inventory")
        );
        assert_eq!(
            resources[0]["mimeType"].as_str().unwrap(),
            "text/html;profile=mcp-app"
        );
    }

    #[test]
    fn does_not_replace_existing_ui_resources() {
        let cfg = PolicyConfig::from_raw(&RawConfig {
            tools: Some(vec![crate::generated::config::Tools0Config {
                name: "get_inventory".into(),
                renderer: None,
                appify: None,
                actions: None,
                csp: None,
                domain: None,
                form_fields: None,
                form_mode: None,
            }]),
            ..raw_empty()
        })
        .unwrap();
        let mut body = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {"resources": [{"uri": "ui://upstream/their-app"}]}
        });
        let _ = rewrite_response(&cfg, &mut body, None);
        // Upstream-served ui:// must not be displaced.
        let resources = body["result"]["resources"].as_array().unwrap();
        assert_eq!(resources.len(), 1);
        assert_eq!(resources[0]["uri"], "ui://upstream/their-app");
    }

    fn raw_empty() -> RawConfig {
        RawConfig {
            appify_tools: None,
            appify_responses: None,
            appify_actions: None,
            renderer: None,
            tools: None,
            default_actions: None,
            deny_tools: None,
            form_tools: None,
            custom_bundles: None,
            csp: None,
            domain: None,
            preview_mode: None,
            debug_headers: None,
            max_body_bytes: None,
        }
    }
}
