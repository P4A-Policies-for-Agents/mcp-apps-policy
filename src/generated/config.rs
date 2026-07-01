use serde::Deserialize;
#[derive(Deserialize, Clone, Debug)]
pub struct CspConfig {
    #[serde(alias = "baseUriDomains")]
    pub base_uri_domains: Option<Vec<String>>,
    #[serde(alias = "connectDomains")]
    pub connect_domains: Option<Vec<String>>,
    #[serde(alias = "frameDomains")]
    pub frame_domains: Option<Vec<String>>,
    #[serde(alias = "resourceDomains")]
    pub resource_domains: Option<Vec<String>>,
}
#[derive(Deserialize, Clone, Debug)]
pub struct Csp1Config {
    #[serde(alias = "baseUriDomains")]
    pub base_uri_domains: Option<Vec<String>>,
    #[serde(alias = "connectDomains")]
    pub connect_domains: Option<Vec<String>>,
    #[serde(alias = "frameDomains")]
    pub frame_domains: Option<Vec<String>>,
    #[serde(alias = "resourceDomains")]
    pub resource_domains: Option<Vec<String>>,
}
#[derive(Deserialize, Clone, Debug)]
pub struct CustomBundles0Config {
    #[serde(alias = "csp")]
    pub csp: Option<Csp1Config>,
    #[serde(alias = "html")]
    pub html: String,
    #[serde(alias = "name")]
    pub name: String,
}
#[derive(Deserialize, Clone, Debug)]
pub struct DefaultActions0Config {
    #[serde(alias = "argsTemplate")]
    pub args_template: Option<String>,
    #[serde(alias = "label")]
    pub label: String,
    #[serde(alias = "mode")]
    pub mode: Option<String>,
    #[serde(alias = "prompt")]
    pub prompt: Option<String>,
    #[serde(alias = "select")]
    pub select: Option<String>,
    #[serde(alias = "tool")]
    pub tool: String,
}
#[derive(Deserialize, Clone, Debug)]
pub struct Actions0Config {
    #[serde(alias = "argsTemplate")]
    pub args_template: Option<String>,
    #[serde(alias = "label")]
    pub label: String,
    #[serde(alias = "mode")]
    pub mode: Option<String>,
    #[serde(alias = "prompt")]
    pub prompt: Option<String>,
    #[serde(alias = "select")]
    pub select: Option<String>,
    #[serde(alias = "tool")]
    pub tool: String,
}
#[derive(Deserialize, Clone, Debug)]
pub struct FormFields0Config {
    #[serde(alias = "fieldType")]
    pub field_type: Option<String>,
    #[serde(alias = "label")]
    pub label: Option<String>,
    #[serde(alias = "name")]
    pub name: String,
    #[serde(alias = "placeholder")]
    pub placeholder: Option<String>,
    #[serde(alias = "required")]
    pub required: Option<bool>,
}
#[derive(Deserialize, Clone, Debug)]
pub struct Tools0Config {
    #[serde(alias = "actions")]
    pub actions: Option<Vec<Actions0Config>>,
    #[serde(alias = "appify")]
    pub appify: Option<bool>,
    #[serde(alias = "csp")]
    pub csp: Option<Csp1Config>,
    #[serde(alias = "domain")]
    pub domain: Option<String>,
    #[serde(alias = "formFields")]
    pub form_fields: Option<Vec<FormFields0Config>>,
    #[serde(alias = "formMode")]
    pub form_mode: Option<String>,
    #[serde(alias = "name")]
    pub name: String,
    #[serde(alias = "renderer")]
    pub renderer: Option<String>,
}
#[derive(Deserialize, Clone, Debug)]
pub struct Config {
    #[serde(alias = "appifyActions")]
    pub appify_actions: Option<bool>,
    #[serde(alias = "appifyResponses")]
    pub appify_responses: Option<bool>,
    #[serde(alias = "appifyTools")]
    pub appify_tools: Option<bool>,
    #[serde(alias = "csp")]
    pub csp: Option<CspConfig>,
    #[serde(alias = "customBundles")]
    pub custom_bundles: Option<Vec<CustomBundles0Config>>,
    #[serde(alias = "debugHeaders")]
    pub debug_headers: Option<bool>,
    #[serde(alias = "defaultActions")]
    pub default_actions: Option<Vec<DefaultActions0Config>>,
    #[serde(alias = "denyTools")]
    pub deny_tools: Option<Vec<String>>,
    #[serde(alias = "domain")]
    pub domain: Option<String>,
    #[serde(alias = "formTools")]
    pub form_tools: Option<Vec<String>>,
    #[serde(alias = "maxBodyBytes")]
    pub max_body_bytes: Option<i64>,
    #[serde(alias = "previewMode")]
    pub preview_mode: Option<bool>,
    #[serde(alias = "renderer")]
    pub renderer: Option<String>,
    #[serde(alias = "tools")]
    pub tools: Option<Vec<Tools0Config>>,
}
#[pdk::hl::entrypoint_flex]
fn init(abi: &dyn pdk::flex_abi::api::FlexAbi) -> Result<(), anyhow::Error> {
    abi.setup()?;
    Ok(())
}
