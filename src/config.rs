//! Typed, validated view over the policy configuration. The raw shape
//! lives in `generated/config.rs`; this module clamps numeric ranges,
//! resolves enums, dedupes deny-list entries, validates that
//! `tools[].renderer` references either a built-in or a `customBundles[]`
//! entry, and normalises defaults.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::generated::config::{
    Actions0Config, Config, Csp1Config, CspConfig, CustomBundles0Config, DefaultActions0Config,
    Tools0Config,
};

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("tool '{0}': {1}")]
    Tool(String, String),
    #[error("custom bundle '{0}': {1}")]
    CustomBundle(String, String),
    #[error("invalid {field}: {value}")]
    Enum { field: &'static str, value: String },
    #[error("default action: {0}")]
    DefaultAction(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Renderer {
    Auto,
    Json,
    Table,
    Card,
    Form,
}

impl Renderer {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim() {
            "auto" => Some(Self::Auto),
            "json" => Some(Self::Json),
            "table" => Some(Self::Table),
            "card" => Some(Self::Card),
            "form" => Some(Self::Form),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Json => "json",
            Self::Table => "table",
            Self::Card => "card",
            Self::Form => "form",
        }
    }
}

/// What a tool's renderer is set to. Either a built-in or the name of
/// a `customBundles[]` entry.
#[derive(Clone)]
pub enum RendererRef {
    BuiltIn(Renderer),
    Custom(String),
}

/// Row-selection requirement for an action button. `None` means the
/// action operates on the top-level result; `Single` requires a radio
/// pick; `Multi` requires ≥ 1 checked rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SelectMode {
    None,
    Single,
    Multi,
}

impl SelectMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim() {
            "" | "none" => Some(Self::None),
            "single" => Some(Self::Single),
            "multi" => Some(Self::Multi),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Single => "single",
            Self::Multi => "multi",
        }
    }
}

/// What clicking the button does. `Call` issues a `tools/call`
/// directly; `Form` opens an inline edit form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ActionMode {
    Call,
    Form,
}

impl ActionMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim() {
            "" | "call" => Some(Self::Call),
            "form" => Some(Self::Form),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Call => "call",
            Self::Form => "form",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Action {
    pub tool: String,
    pub label: String,
    /// Parsed JSON template. `None` = "call the tool with empty args".
    /// For `select == None` the policy substitutes `${field}` against
    /// `structuredContent` server-side at response time. For
    /// `single`/`multi` the template ships through to the bundle as-is
    /// so substitution can happen against the user's row selection.
    pub args_template: Option<serde_json::Value>,
    pub select: SelectMode,
    pub mode: ActionMode,
}

/// CSP allowlists emitted as `_meta.ui.csp`. Same shape used at the
/// policy level, per-tool, and on `customBundles[]`. Empty arrays are
/// valid — they tell the host "no extra origins beyond defaults".
#[derive(Debug, Clone, Default)]
pub struct CspBlock {
    pub connect_domains: Vec<String>,
    pub resource_domains: Vec<String>,
    pub frame_domains: Vec<String>,
    pub base_uri_domains: Vec<String>,
}

impl CspBlock {
    pub fn is_empty(&self) -> bool {
        self.connect_domains.is_empty()
            && self.resource_domains.is_empty()
            && self.frame_domains.is_empty()
            && self.base_uri_domains.is_empty()
    }

    pub fn to_meta(&self) -> serde_json::Value {
        serde_json::json!({
            "connectDomains": self.connect_domains,
            "resourceDomains": self.resource_domains,
            "frameDomains": self.frame_domains,
            "baseUriDomains": self.base_uri_domains,
        })
    }
}

/// Back-compat alias — `bundle::html_for` and earlier code refer to
/// `CustomBundleCsp`.
pub type CustomBundleCsp = CspBlock;

#[derive(Debug, Clone)]
pub struct ToolOverride {
    pub renderer: Option<RendererRef>,
    pub appify: bool,
    pub actions: Vec<Action>,
    /// Per-tool `_meta.ui.domain`. `None` means "fall back to the
    /// global default, then to a synthesised
    /// `<toolName>.mcp-apps-policy.local`."
    pub domain: Option<String>,
    /// Per-tool CSP. `None` means "use the global default".
    pub csp: Option<CspBlock>,
}

#[derive(Debug, Clone)]
pub struct CustomBundle {
    pub name: String,
    pub html: String,
    pub csp: CspBlock,
}

#[derive(Debug, Clone)]
pub struct PolicyConfig {
    pub appify_tools: bool,
    pub appify_responses: bool,
    pub appify_actions: bool,
    pub renderer: Renderer,
    pub tools: HashMap<String, ToolOverride>,
    pub default_actions: Vec<Action>,
    pub deny_tools: HashSet<String>,
    pub custom_bundles: HashMap<String, CustomBundle>,
    /// Default `_meta.ui.csp` block applied when a tool doesn't carry
    /// its own override.
    pub csp: CspBlock,
    /// Default `_meta.ui.domain`. Empty string means "synthesise per
    /// tool from the tool name".
    pub domain: String,
    pub preview_mode: bool,
    pub debug_headers: bool,
    pub max_body_bytes: usize,
}

impl PolicyConfig {
    pub fn from_raw(raw: &Config) -> Result<Self, ConfigError> {
        let renderer =
            parse_enum("renderer", raw.renderer.as_deref(), "auto", Renderer::parse)?;

        let custom_bundles = raw
            .custom_bundles
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .map(parse_custom_bundle)
            .collect::<Result<Vec<_>, _>>()?;
        let custom_bundles_idx: HashMap<String, CustomBundle> =
            custom_bundles.into_iter().map(|b| (b.name.clone(), b)).collect();

        let default_actions = raw
            .default_actions
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .map(parse_default_action)
            .collect::<Result<Vec<_>, _>>()?;

        let deny_tools: HashSet<String> = raw
            .deny_tools
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        let tools = raw
            .tools
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .map(|t| parse_tool(t, &custom_bundles_idx))
            .collect::<Result<Vec<_>, _>>()?;
        let tools_idx: HashMap<String, ToolOverride> =
            tools.into_iter().collect();

        let max_body_bytes = raw
            .max_body_bytes
            .filter(|v| (1024..=52_428_800).contains(v))
            .map(|v| v as usize)
            .unwrap_or(1_048_576);

        let csp = raw
            .csp
            .as_ref()
            .map(parse_top_level_csp)
            .unwrap_or_default();
        let domain = raw
            .domain
            .as_deref()
            .map(|s| s.trim().to_string())
            .unwrap_or_default();

        Ok(Self {
            appify_tools: raw.appify_tools.unwrap_or(true),
            appify_responses: raw.appify_responses.unwrap_or(true),
            appify_actions: raw.appify_actions.unwrap_or(true),
            renderer,
            tools: tools_idx,
            default_actions,
            deny_tools,
            custom_bundles: custom_bundles_idx,
            csp,
            domain,
            preview_mode: raw.preview_mode.unwrap_or(false),
            debug_headers: raw.debug_headers.unwrap_or(false),
            max_body_bytes,
        })
    }

    /// Resolve the `_meta.ui.domain` for a given tool.
    /// Per-tool override → top-level default → synthesised
    /// `<toolName>.mcp-apps-policy.local` (sanitised — non-DNS chars
    /// become `-`).
    pub fn domain_for(&self, tool: &str) -> String {
        if let Some(t) = self.tools.get(tool) {
            if let Some(d) = &t.domain {
                if !d.is_empty() {
                    return d.clone();
                }
            }
        }
        if !self.domain.is_empty() {
            return self.domain.clone();
        }
        format!("{}.mcp-apps-policy.local", sanitise_dns_label(tool))
    }

    /// Resolve the `_meta.ui.csp` block for a given tool. Per-tool
    /// override wins entirely; otherwise the global default applies.
    pub fn csp_for(&self, tool: &str) -> &CspBlock {
        if let Some(t) = self.tools.get(tool) {
            if let Some(c) = &t.csp {
                return c;
            }
        }
        &self.csp
    }

    /// Resolve the renderer to use for a given tool, considering the
    /// per-tool override and the policy default.
    pub fn renderer_for(&self, tool: &str) -> RendererRef {
        if let Some(t) = self.tools.get(tool) {
            if let Some(r) = &t.renderer {
                return r.clone();
            }
        }
        RendererRef::BuiltIn(self.renderer)
    }

    /// Returns true when the tool should be advertised as an app and
    /// rendered through the policy. Centralises the deny-list /
    /// per-tool `appify=false` logic.
    pub fn appifies(&self, tool: &str) -> bool {
        if !self.appify_tools {
            return false;
        }
        if self.deny_tools.contains(tool) {
            return false;
        }
        self.tools
            .get(tool)
            .map(|t| t.appify)
            .unwrap_or(true)
    }

    /// Effective action list for a tool: per-tool actions first, then
    /// `defaultActions`. Both are dropped when `appify_actions` is off
    /// or the tool is on the deny-list.
    pub fn actions_for(&self, tool: &str) -> Vec<Action> {
        if !self.appify_actions || self.deny_tools.contains(tool) {
            return Vec::new();
        }
        let mut out: Vec<Action> = Vec::new();
        if let Some(t) = self.tools.get(tool) {
            out.extend(t.actions.iter().cloned());
        }
        out.extend(self.default_actions.iter().cloned());
        out.into_iter()
            .filter(|a| !self.deny_tools.contains(&a.tool))
            .collect()
    }
}

fn parse_enum<T>(
    field: &'static str,
    value: Option<&str>,
    default: &str,
    parse: impl Fn(&str) -> Option<T>,
) -> Result<T, ConfigError> {
    let v = value
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .unwrap_or(default);
    parse(v).ok_or_else(|| ConfigError::Enum {
        field,
        value: v.to_string(),
    })
}

fn parse_tool(
    raw: &Tools0Config,
    custom_bundles: &HashMap<String, CustomBundle>,
) -> Result<(String, ToolOverride), ConfigError> {
    let name = raw.name.trim().to_string();
    if name.is_empty() {
        return Err(ConfigError::Tool(
            "<unnamed>".into(),
            "missing 'name'".into(),
        ));
    }
    let renderer = match raw
        .renderer
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        None => None,
        Some(s) => match Renderer::parse(s) {
            Some(b) => Some(RendererRef::BuiltIn(b)),
            None => {
                if !custom_bundles.contains_key(s) {
                    return Err(ConfigError::Tool(
                        name,
                        format!(
                            "renderer '{s}' is neither a built-in (auto|json|table|card|form) nor a customBundles[] name"
                        ),
                    ));
                }
                Some(RendererRef::Custom(s.to_string()))
            }
        },
    };
    let appify = raw.appify.unwrap_or(true);
    let actions = raw
        .actions
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .map(|a| parse_action(&name, a))
        .collect::<Result<Vec<_>, _>>()?;
    let domain = raw
        .domain
        .as_deref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let csp = raw.csp.as_ref().map(parse_per_tool_csp);
    Ok((
        name,
        ToolOverride {
            renderer,
            appify,
            actions,
            domain,
            csp,
        },
    ))
}

fn parse_action(rule: &str, raw: &Actions0Config) -> Result<Action, ConfigError> {
    let tool = raw.tool.trim().to_string();
    if tool.is_empty() {
        return Err(ConfigError::Tool(
            rule.into(),
            "action with empty 'tool'".into(),
        ));
    }
    let label = raw.label.trim().to_string();
    if label.is_empty() {
        return Err(ConfigError::Tool(
            rule.into(),
            format!("action -> {tool}: empty 'label'"),
        ));
    }
    let args_template = parse_args_template(&raw.args_template)
        .map_err(|e| ConfigError::Tool(rule.into(), format!("action -> {tool}: {e}")))?;
    let select = parse_select(&raw.select)
        .map_err(|e| ConfigError::Tool(rule.into(), format!("action -> {tool}: {e}")))?;
    let mode = parse_action_mode(&raw.mode)
        .map_err(|e| ConfigError::Tool(rule.into(), format!("action -> {tool}: {e}")))?;
    Ok(Action {
        tool,
        label,
        args_template,
        select,
        mode,
    })
}

fn parse_default_action(raw: &DefaultActions0Config) -> Result<Action, ConfigError> {
    let tool = raw.tool.trim().to_string();
    if tool.is_empty() {
        return Err(ConfigError::DefaultAction(
            "missing 'tool'".into(),
        ));
    }
    let label = raw.label.trim().to_string();
    if label.is_empty() {
        return Err(ConfigError::DefaultAction(
            format!("{tool}: empty 'label'"),
        ));
    }
    let args_template = parse_args_template(&raw.args_template)
        .map_err(|e| ConfigError::DefaultAction(format!("{tool}: {e}")))?;
    let select = parse_select(&raw.select)
        .map_err(|e| ConfigError::DefaultAction(format!("{tool}: {e}")))?;
    let mode = parse_action_mode(&raw.mode)
        .map_err(|e| ConfigError::DefaultAction(format!("{tool}: {e}")))?;
    Ok(Action {
        tool,
        label,
        args_template,
        select,
        mode,
    })
}

fn parse_select(raw: &Option<String>) -> Result<SelectMode, String> {
    let s = raw.as_deref().unwrap_or("");
    SelectMode::parse(s).ok_or_else(|| format!("invalid select '{s}' (use none|single|multi)"))
}

fn parse_action_mode(raw: &Option<String>) -> Result<ActionMode, String> {
    let s = raw.as_deref().unwrap_or("");
    ActionMode::parse(s).ok_or_else(|| format!("invalid mode '{s}' (use call|form)"))
}

/// Map a tool name to a DNS label by replacing characters illegal in
/// DNS with `-`, lowercasing, collapsing runs of `-`, and trimming.
fn sanitise_dns_label(tool: &str) -> String {
    let mut out = String::with_capacity(tool.len());
    let mut last_dash = false;
    for c in tool.chars() {
        let ok = c.is_ascii_alphanumeric();
        if ok {
            out.push(c.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

fn parse_args_template(
    raw: &Option<String>,
) -> Result<Option<serde_json::Value>, String> {
    match raw.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        None => Ok(None),
        Some(s) => serde_json::from_str(s)
            .map(Some)
            .map_err(|e| format!("argsTemplate is not valid JSON: {e}")),
    }
}

fn parse_custom_bundle(raw: &CustomBundles0Config) -> Result<CustomBundle, ConfigError> {
    let name = raw.name.trim().to_string();
    if name.is_empty() {
        return Err(ConfigError::CustomBundle(
            "<unnamed>".into(),
            "missing 'name'".into(),
        ));
    }
    if Renderer::parse(&name).is_some() {
        return Err(ConfigError::CustomBundle(
            name.clone(),
            "name shadows a built-in renderer (auto|json|table|card|form)".into(),
        ));
    }
    let html = raw.html.clone();
    if html.trim().is_empty() {
        return Err(ConfigError::CustomBundle(
            name,
            "missing 'html'".into(),
        ));
    }
    let csp = match &raw.csp {
        None => CustomBundleCsp {
            connect_domains: Vec::new(),
            resource_domains: Vec::new(),
            frame_domains: Vec::new(),
            base_uri_domains: Vec::new(),
        },
        Some(c) => parse_csp(c),
    };
    Ok(CustomBundle { name, html, csp })
}

fn collect_domains(v: &Option<Vec<String>>) -> Vec<String> {
    v.as_deref()
        .unwrap_or(&[])
        .iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn parse_csp(raw: &Csp1Config) -> CspBlock {
    CspBlock {
        connect_domains: collect_domains(&raw.connect_domains),
        resource_domains: collect_domains(&raw.resource_domains),
        frame_domains: collect_domains(&raw.frame_domains),
        base_uri_domains: collect_domains(&raw.base_uri_domains),
    }
}

fn parse_per_tool_csp(raw: &Csp1Config) -> CspBlock {
    parse_csp(raw)
}

fn parse_top_level_csp(raw: &CspConfig) -> CspBlock {
    CspBlock {
        connect_domains: collect_domains(&raw.connect_domains),
        resource_domains: collect_domains(&raw.resource_domains),
        frame_domains: collect_domains(&raw.frame_domains),
        base_uri_domains: collect_domains(&raw.base_uri_domains),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generated::config::{
        Actions0Config, Config, CustomBundles0Config, Tools0Config,
    };

    fn empty_config() -> Config {
        Config {
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
    fn defaults_are_sensible() {
        let cfg = PolicyConfig::from_raw(&empty_config()).unwrap();
        assert!(cfg.appify_tools);
        assert!(cfg.appify_responses);
        assert!(cfg.appify_actions);
        assert_eq!(cfg.renderer, Renderer::Auto);
        assert_eq!(cfg.max_body_bytes, 1_048_576);
        assert!(cfg.tools.is_empty());
        assert!(cfg.deny_tools.is_empty());
    }

    #[test]
    fn deny_list_blocks_appify() {
        let cfg = PolicyConfig::from_raw(&Config {
            deny_tools: Some(vec!["secret_admin".into()]),
            ..empty_config()
        })
        .unwrap();
        assert!(cfg.appifies("get_inventory"));
        assert!(!cfg.appifies("secret_admin"));
    }

    #[test]
    fn rejects_unknown_renderer_name() {
        let err = PolicyConfig::from_raw(&Config {
            tools: Some(vec![Tools0Config {
                name: "x".into(),
                renderer: Some("nonexistent".into()),
                appify: None,
                actions: None,
                csp: None,
                domain: None,
            }]),
            ..empty_config()
        })
        .unwrap_err();
        assert!(format!("{err}").contains("nonexistent"));
    }

    #[test]
    fn accepts_custom_bundle_renderer_reference() {
        let cfg = PolicyConfig::from_raw(&Config {
            custom_bundles: Some(vec![CustomBundles0Config {
                name: "fancy".into(),
                html: "<!doctype html><html></html>".into(),
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
            ..empty_config()
        })
        .unwrap();
        match cfg.renderer_for("x") {
            RendererRef::Custom(n) => assert_eq!(n, "fancy"),
            other => panic!("expected custom bundle renderer, got {other:?}"),
        }
    }

    #[test]
    fn custom_bundle_cannot_shadow_builtin_name() {
        let err = PolicyConfig::from_raw(&Config {
            custom_bundles: Some(vec![CustomBundles0Config {
                name: "json".into(),
                html: "<!doctype html><html></html>".into(),
                csp: None,
            }]),
            ..empty_config()
        })
        .unwrap_err();
        assert!(format!("{err}").to_lowercase().contains("shadow"));
    }

    #[test]
    fn parses_actions_with_args_template() {
        let cfg = PolicyConfig::from_raw(&Config {
            tools: Some(vec![Tools0Config {
                name: "get_inventory".into(),
                renderer: None,
                appify: None,
                actions: Some(vec![Actions0Config {
                    tool: "create_order".into(),
                    label: "Order".into(),
                    args_template: Some("{\"sku\":\"${sku}\"}".into()),
                    select: None,
                    mode: None,
                }]),
                csp: None,
                domain: None,
            }]),
            ..empty_config()
        })
        .unwrap();
        let actions = cfg.actions_for("get_inventory");
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].tool, "create_order");
        assert!(actions[0].args_template.is_some());
    }

    #[test]
    fn deny_list_filters_actions_by_target() {
        let cfg = PolicyConfig::from_raw(&Config {
            deny_tools: Some(vec!["forbidden".into()]),
            tools: Some(vec![Tools0Config {
                name: "get_inventory".into(),
                renderer: None,
                appify: None,
                actions: Some(vec![
                    Actions0Config {
                        tool: "ok_tool".into(),
                        label: "OK".into(),
                        args_template: None,
                        select: None,
                        mode: None,
                    },
                    Actions0Config {
                        tool: "forbidden".into(),
                        label: "No".into(),
                        args_template: None,
                        select: None,
                        mode: None,
                    },
                ]),
                csp: None,
                domain: None,
            }]),
            ..empty_config()
        })
        .unwrap();
        let actions = cfg.actions_for("get_inventory");
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].tool, "ok_tool");
    }

    #[test]
    fn rejects_invalid_args_template_json() {
        let err = PolicyConfig::from_raw(&Config {
            tools: Some(vec![Tools0Config {
                name: "x".into(),
                renderer: None,
                appify: None,
                actions: Some(vec![Actions0Config {
                    tool: "y".into(),
                    label: "Y".into(),
                    args_template: Some("{ not json".into()),
                    select: None,
                    mode: None,
                }]),
                csp: None,
                domain: None,
            }]),
            ..empty_config()
        })
        .unwrap_err();
        assert!(format!("{err}").contains("argsTemplate"));
    }
}

// Manual Debug impl for RendererRef so the test panic above renders.
impl std::fmt::Debug for RendererRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BuiltIn(r) => write!(f, "BuiltIn({})", r.as_str()),
            Self::Custom(n) => write!(f, "Custom({n})"),
        }
    }
}
