//! Embedded HTML bundle served when the MCP host requests a
//! `ui://mcp-apps-policy/<tool>` resource.
//!
//! One auto-rendering vanilla-JS bundle covers every tool: it
//! handshakes via `ui/initialize`, listens for
//! `ui/notifications/tool-result`, inspects the structured payload's
//! shape (object → card, array of objects → table, otherwise JSON),
//! and renders buttons for any `_meta.ui.actions[]` the policy
//! injected. Buttons issue `tools/call` through the host so each
//! action opens the next tool's app inline — no per-tool code needed.

use serde_json::{json, Value};

use crate::config::{CustomBundle, PolicyConfig};

/// The compiled-in HTML for the default auto-renderer. Bundled at
/// compile time so the policy is self-contained.
pub const AUTO_HTML: &str = include_str!("auto.html");

/// Returns the HTML bytes the policy should serve for a given
/// `ui://mcp-apps-policy/<tool>` URI together with the `_meta.ui` JSON
/// to attach (CSP / permissions). When no custom bundle is configured
/// for the tool, falls back to the auto bundle.
pub fn html_for(cfg: &PolicyConfig, tool_name: &str) -> (String, Value) {
    if let Some(bundle) = custom_bundle_for_tool(cfg, tool_name) {
        let mut meta = json!({});
        if !bundle.csp.is_empty() {
            if let Some(map) = meta.as_object_mut() {
                map.insert("csp".into(), bundle.csp.to_meta());
            }
        }
        return (bundle.html.clone(), meta);
    }
    // Default bundle: keep CSP permissive enough to talk to the host
    // (postMessage doesn't need network), but don't grant any extra
    // origins. Hosts will fall back to the spec's default restrictive
    // CSP, which is what we want.
    let html = if cfg.preview_mode {
        // Inject a marker the bundle's debug overlay reads to enable
        // itself. We can't append a query string because hosts (Claude's
        // sandbox proxy) re-host the HTML on their own origin.
        AUTO_HTML.replacen(
            "<head>",
            "<head>\n<meta name=\"x-mcp-debug\" content=\"1\">",
            1,
        )
    } else {
        AUTO_HTML.to_string()
    };
    (html, json!({}))
}

fn custom_bundle_for_tool<'a>(
    cfg: &'a PolicyConfig,
    tool_name: &str,
) -> Option<&'a CustomBundle> {
    let renderer_ref = cfg.renderer_for(tool_name);
    let crate::config::RendererRef::Custom(name) = renderer_ref else {
        return None;
    };
    cfg.custom_bundles.get(&name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generated::config::{Config as RawConfig, CustomBundles0Config, Tools0Config};

    fn empty_raw() -> RawConfig {
        RawConfig {
            appify_tools: None,
            appify_responses: None,
            appify_actions: None,
            renderer: None,
            tools: None,
            default_actions: None,
            deny_tools: None,
            custom_bundles: None,
            csp: None,
            domain: None,
            preview_mode: None,
            debug_headers: None,
            max_body_bytes: None,
        }
    }

    #[test]
    fn auto_html_is_a_full_doctype() {
        assert!(AUTO_HTML.starts_with("<!DOCTYPE html>"));
        assert!(AUTO_HTML.contains("ui/initialize"));
        assert!(AUTO_HTML.contains("ui/notifications/tool-result"));
        assert!(AUTO_HTML.contains("tools/call"));
    }

    #[test]
    fn defaults_to_auto_bundle() {
        let cfg = PolicyConfig::from_raw(&empty_raw()).unwrap();
        let (html, meta) = html_for(&cfg, "anything");
        assert_eq!(html, AUTO_HTML);
        assert!(meta.as_object().unwrap().is_empty());
    }

    #[test]
    fn returns_custom_bundle_when_referenced() {
        let cfg = PolicyConfig::from_raw(&RawConfig {
            custom_bundles: Some(vec![CustomBundles0Config {
                name: "fancy".into(),
                html: "<!doctype html><html>fancy</html>".into(),
                csp: None,
            }]),
            tools: Some(vec![Tools0Config {
                name: "x".into(),
                renderer: Some("fancy".into()),
                appify: None,
                actions: None,
                csp: None,
                domain: None,
            }]),
            ..empty_raw()
        })
        .unwrap();
        let (html, _) = html_for(&cfg, "x");
        assert!(html.contains("fancy"));
    }
}
