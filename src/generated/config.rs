//! Hand-authored config types matching `definition/gcl.yaml`.
//!
//! `cargo anypoint config-gen` regenerates this file at build time
//! from the GCL. Keep the field set in sync with the GCL when editing
//! either source. The shape is intentionally permissive (every leaf is
//! `Option<T>`) so older policy configs keep loading after we add new
//! fields.

use serde::Deserialize;

#[derive(Deserialize, Clone, Debug)]
pub struct Actions0Config {
    #[serde(alias = "tool")]
    pub tool: String,
    #[serde(alias = "label")]
    pub label: String,
    #[serde(alias = "argsTemplate")]
    pub args_template: Option<String>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Tools0Config {
    #[serde(alias = "name")]
    pub name: String,
    #[serde(alias = "renderer")]
    pub renderer: Option<String>,
    #[serde(alias = "appify")]
    pub appify: Option<bool>,
    #[serde(alias = "actions")]
    pub actions: Option<Vec<Actions0Config>>,
}

#[derive(Deserialize, Clone, Debug, Default)]
pub struct CustomBundleCspConfig {
    #[serde(alias = "connectDomains")]
    pub connect_domains: Option<Vec<String>>,
    #[serde(alias = "resourceDomains")]
    pub resource_domains: Option<Vec<String>>,
    #[serde(alias = "frameDomains")]
    pub frame_domains: Option<Vec<String>>,
    #[serde(alias = "baseUriDomains")]
    pub base_uri_domains: Option<Vec<String>>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct CustomBundles0Config {
    #[serde(alias = "name")]
    pub name: String,
    #[serde(alias = "html")]
    pub html: String,
    #[serde(alias = "csp")]
    pub csp: Option<CustomBundleCspConfig>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Config {
    #[serde(alias = "appifyTools")]
    pub appify_tools: Option<bool>,
    #[serde(alias = "appifyResponses")]
    pub appify_responses: Option<bool>,
    #[serde(alias = "appifyActions")]
    pub appify_actions: Option<bool>,
    #[serde(alias = "renderer")]
    pub renderer: Option<String>,
    #[serde(alias = "tools")]
    pub tools: Option<Vec<Tools0Config>>,
    #[serde(alias = "defaultActions")]
    pub default_actions: Option<Vec<Actions0Config>>,
    #[serde(alias = "denyTools")]
    pub deny_tools: Option<Vec<String>>,
    #[serde(alias = "customBundles")]
    pub custom_bundles: Option<Vec<CustomBundles0Config>>,
    #[serde(alias = "previewMode")]
    pub preview_mode: Option<bool>,
    #[serde(alias = "debugHeaders")]
    pub debug_headers: Option<bool>,
    #[serde(alias = "maxBodyBytes")]
    pub max_body_bytes: Option<i64>,
}

#[pdk::hl::entrypoint_flex]
fn init(abi: &dyn pdk::flex_abi::api::FlexAbi) -> Result<(), anyhow::Error> {
    abi.setup()?;
    Ok(())
}
