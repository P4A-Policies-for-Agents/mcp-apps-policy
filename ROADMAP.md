# Roadmap

This roadmap is the policy's living plan. It tracks what shipped,
what is under active design, and what remains exploratory.

---

## v0.1 — Public Preview *(current)*

The first release covers the wire-format work — discovery, response
shaping, action injection, SSE — and ships a self-contained UI
bundle so the policy works without external hosting.

**Shipped**

- **Three master switches.** `appifyTools`, `appifyResponses`,
  `appifyActions` each toggle one transform independently.
- **`tools/list` annotation.** Every advertised tool gets
  `_meta.ui.resourceUri = "ui://mcp-apps-policy/<encoded-name>"` plus
  a corresponding entry in `resources/list`.
- **`resources/read` short-circuit.** `ui://mcp-apps-policy/<tool>`
  URIs are answered locally with the embedded HTML bundle; the
  upstream is never called.
- **`tools/call` shaping.** JSON-bearing `content[0].text` is parsed
  and copied into `result.structuredContent` (existing
  `structuredContent` is preserved).
- **Action injection.** `_meta.ui.actions[]` is populated from the
  matching `tools[]` / `defaultActions` entries; `${field}`
  placeholders in `argsTemplate` are substituted from the current
  result's `structuredContent` at click time, inside the iframe.
- **Embedded auto-rendering bundle.** A single self-contained HTML5
  document (~10 KB of vanilla JS) implements `ui/initialize`,
  `ui/notifications/initialized`, listens for
  `ui/notifications/tool-result`, picks a layout from the result's
  shape (table / card / form / json), draws action buttons from
  `_meta.ui.actions`, and reports `ui/notifications/size-changed`.
- **Custom bundles.** Operators can register inline HTML and a CSP
  block; per-tool `renderer: <name>` opts a tool into a custom bundle
  instead of the built-in.
- **Deny list.** `denyTools[]` excludes named tools from
  appification, advertisement, and action targeting.
- **Streamable HTTP / SSE.** MCP servers responding with
  `Content-Type: text/event-stream` are handled. Each `data:` JSON
  frame is parsed, transformed through the same pipeline as plain
  JSON, and re-emitted as a valid SSE event. Heartbeats and non-JSON
  frames pass through.
- **Body-size cap.** `maxBodyBytes` (default 1 MiB, max 50 MiB) with
  pass-through above the cap.
- **Diagnostics.** `previewMode` (`x-mcp-apps-preview` header) and
  `debugHeaders` (`x-mcp-apps-method` / `-tool` / `-action` /
  `-renderer`).
- **Integration test harness on `pdk-unit`** covering `tools/list`,
  `tools/call`, `resources/read`, and SSE shapes.
- **Request-side tool-name capture (0.1.1).** `on_request` parses the
  `tools/call` body and stashes `params.name` in `RequestState`; the
  response phase reads it so action injection works against upstreams
  that don't echo `_meta.toolName` (which is most of them).
- **`structuredContent` array wrap (0.1.4).** The MCP spec requires
  `structuredContent` to be a JSON object. When the upstream's
  `content[0].text` parses to an array (e.g. CRM `get_accounts`),
  the policy now wraps it under `{"items": [...]}` so the response
  is spec-valid; the embedded bundle unwraps that shape on render so
  the table view still kicks in.
- **In-iframe debug overlay (0.1.5–0.1.6).** When `previewMode: true`,
  `bundle::html_for` injects `<meta name="x-mcp-debug" content="1">`
  into the served HTML and the bundle attaches a fixed-position log
  of every postMessage in/out plus current state. Lets us diagnose
  the SEP-1865 handshake from inside hosts whose iframe console is
  awkward to open (e.g. Claude.ai's sandbox proxy).
- **SEP-1865 handshake correctness (0.1.6–0.1.8).** The `ui/initialize`
  payload now sends `displayModes: ["inline", "fullscreen"]` (replacing
  an invalid `tools.listChanged` capability) and uses the spec-correct
  `appInfo` field name (was `clientInfo`, which strict hosts rejected
  with `params.appInfo: invalid_type`). Early `size-changed` reports
  fire on DOMContentLoaded / first RAF so hosts that gate iframe
  visibility on a non-zero size reveal the bundle even before the first
  tool result arrives.
- **Per-server configs.** The repo ships `policy-config.crm.json` and
  `policy-config.erp.json` so each upstream gets only its own tools in
  `tools[]`. A single shared config caused phantom `ui://` resources
  to leak across servers (CRM tools showing up on the ERP MCP and
  vice versa); per-upstream files keep the resource list scoped.
- **Versioned `ui://` URIs (0.1.9).** Synthesised URIs now include the
  policy version: `ui://mcp-apps-policy/v<version>/<tool>`. Claude.ai's
  `*.claudemcpcontent.com` sandbox proxy caches the bundle bytes by
  URI and otherwise pins the first bundle it ever fetches — when we
  shipped the corrected `appInfo` handshake in 0.1.8 it kept serving
  the v0.1.6 bundle for existing conversations, which the host then
  rejected with `params.appInfo: invalid_type`. Each release now bumps
  the URI segment so the cache misses and the new bundle is fetched.
  The legacy unversioned shape (`ui://mcp-apps-policy/<tool>`) is
  still accepted on the read path so cached references from older
  releases keep resolving.
- **Spec-namespaced `_meta` key (0.1.10).** UI metadata is now emitted
  under both `_meta["io.modelcontextprotocol/ui"]` (the SEP-1865
  spec-namespaced key) and `_meta.ui` (the relaxed alias used by
  Inspector and the embedded bundle). Strict hosts — Claude.ai
  rejected previous releases with *"Tool X has no UI resource (no
  ui/resourceUri in tool._meta)"* because they only look under the
  fully-qualified key, while the alias kept relaxed hosts working.
  Dual-write covers both groups without forcing operators to pick.
  The embedded bundle reads whichever shape is present.

**Known gaps**

- The auto-rendering bundle is shape-driven and substitute-only;
  templates that internally compose lists+cards aren't supported.
  Workaround: a custom bundle.
- Action injection draws actions from config, not from the tool's
  `outputSchema`. Schema-driven scaffolding is on v0.2.
- One transform per response — multi-rule composition is out of
  scope for v0.1.
- SSE transformation is per-event with no cross-event state. Fine
  for current MCP responses (one frame per RPC) but not for
  protocols that split a single logical document across frames.

---

## v0.2 — Schema-driven scaffolding *(planned)*

Goal: reduce config surface for operators by reading what the MCP
server already advertises.

- **Schema-driven scaffolding.** Read `tools[].inputSchema` and
  `tools[].outputSchema` to pick the renderer (form for inputs with
  required fields, table for array outputs, card for object outputs)
  and to derive sensible defaults for `argsTemplate`.
- **Action discovery from MCP relations.** Once MCP gains a standard
  for "next tool" hints (today this is in flux), wire those into
  `_meta.ui.actions` automatically and treat `tools[].actions[]` as
  overrides.
- **Header-driven renderer negotiation.** Honour a request-side
  `X-MCP-Apps-Renderer: <name>` so an operator can A/B different
  bundles per route without editing config.
- **Per-rule rate caps.** Optional cap on transformations per second
  per tool, to protect the policy itself during incidents.
- **Bundle hot-reload from a sibling Exchange asset.** Today bundles
  ship inside the WASM artifact. v0.2 adds an opt-in path that
  fetches a versioned bundle from Exchange so the iframe code can
  iterate without re-publishing the policy.

---

## v0.3 — Beyond UI annotation *(exploratory)*

- **Bidirectional iframe state.** `ui/notifications/state-changed`
  bubbled up to the host so app state survives navigation.
- **Pluggable telemetry.** Prometheus exporter via PDK metrics, plus
  OTLP traces around the response phase.
- **Multi-tool composition.** Allow more than one tool to contribute
  fragments to a single rendered document (e.g. summary card +
  detail table) without forcing everything through a single bundle.
- **Bundle packs.** Distribute curated UI bundles per upstream class
  (Salesforce CRM, Zendesk, Stripe, …) as Exchange assets that
  import cleanly into a deployment's `customBundles[]`.
- **Cross-policy alignment.** A2UI ([`a2ui-policy`](../a2ui-policy))
  and MCP Apps share the "render structured data deterministically"
  premise. Open question: ship a thin shim so a single rule set can
  drive both surfaces, or keep them as siblings that operators choose
  between?

---

## Open questions

These are not blockers — they're decisions the team wants more signal
on before committing.

- **`ui://` authority.** `mcp-apps-policy` is fixed today so the
  policy can recognise its own URIs. Should this be operator-tunable
  for multi-tenant deployments?
- **Bundle distribution.** Should bundles always live in the WASM
  artifact (cheap, atomic versioning) or should we support
  side-loading from Exchange / a CDN (cheaper iteration, weaker
  versioning)?
- **`appifyResponses` default.** Today copying JSON-bearing
  `content[0].text` into `structuredContent` is on by default.
  Strictly, the MCP spec leaves `content` as the model-facing slot
  and `structuredContent` as the UI-facing slot — operators may want
  to opt in instead of opt out.
- **Custom bundle CSP defaults.** When an operator omits `csp`, we
  emit nothing and let the host pick a default. Should the policy
  inject a tight default (`connect-src 'none'`) instead?
- **Client rendering gap.** Stock MCP hosts (Claude Desktop, ChatGPT,
  Gemini, Cursor) implement MCP Apps inconsistently — some render
  the iframe, some inline-quote `structuredContent`, some ignore
  `_meta.ui` entirely. Options: ship a reference MCP-Apps-aware demo
  client, document a "shadow text" rendering hint the LLM can lean
  on, or wait for client-side support to land.

Feedback on any of these via issues / Slack is appreciated.
