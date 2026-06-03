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
- **CSP + domain emission (0.1.11).** Every appified tool now carries
  `_meta.ui.csp = {connectDomains, resourceDomains, frameDomains,
  baseUriDomains}` and `_meta.ui.domain` (synthesised from the tool
  name when not configured). ChatGPT's app-submission validator
  rejects templates without these — *"Widget CSP is not set for this
  template"* and *"Widget domain is not set for this template"*.
  Both fields are configurable globally (`csp`, `domain`) and
  per-tool (`tools[].csp`, `tools[].domain`); per-tool replaces
  global entirely. Claude.ai ignores the new fields, so the change
  is purely additive on that host.
- **Working table actions (0.1.11).** Actions now have `select`
  (`none`/`single`/`multi`) and `mode` (`call`/`form`) attributes.
  The embedded bundle renders a radio column for `select: single`
  and a checkbox column for `select: multi`; action buttons are
  disabled until the requirement is met. Substitution moved
  client-side for selection-bound actions: `${field}` reads from
  the picked row, `${row}` is the whole row, `${rows}` is the
  selected array, and `${rows[].Field}` projects a field across
  rows. `mode: form` (single-select only) opens an inline edit form
  built from the row's keys, hides system fields (`Id`, timestamps),
  and submits the edited row as `tools/call` arguments. Closes the
  long-standing gap where Edit / Delete buttons fired with empty
  `Id`s because substitution happened against the top-level result.
- **Inspector compatibility (0.1.11).** The bundle now captures the
  host's origin from the first inbound JSON-RPC frame and pins
  `postMessage` `targetOrigin` to it. Pre-0.1.11 we sent every
  message with `targetOrigin: "*"`, which MCP Inspector's strict
  cross-origin isolation could drop.
- **Form Submit posts via `ui/message`, plus `formMode: skip` opt-out
  (0.1.30).** Issuing `tools/call` from inside the iframe doesn't
  work on every host — Claude.ai never delivers the result back, so
  the form would hang on `MCP error -32001: Request timed out`. New
  Submit path posts a chat prompt via SEP-1865 `ui/message` asking
  the agent to call the tool with the merged args plus
  `_mcpAppsConfirmed: true`; the policy strips the marker and
  forwards upstream, the host pushes the tool-result back through
  the normal channel. Same path Edit/Delete `prompt`-mode actions
  use today, so works on every host. Also adds per-tool
  `formMode: "auto" | "skip"`. `skip` opts the tool out of
  `formTools[]` interception entirely — set on `submit_order` in
  the ERP demo because the upstream `get_inventory` "Order this
  material" action already gathers consent in chat, so a second
  form is redundant noise.
- **Revert fire-and-replace form submit; revert CRM Edit to prompt
  mode (0.1.29).** 0.1.27's fire-and-replace logic for the pre-call
  form (`formTools[]`) and the single-row inline Edit form turned
  out to misdiagnose the host: on this user's host the iframe-side
  `tools/call` does receive a JSON-RPC reply (so the prior await
  path worked), and the new "Submitted — waiting…" notice was
  staying stuck because the host *also* funnels confirmed calls
  through the agent which then re-issues the call as a fresh chat
  turn. Restored the original `fireToolsCall().then(close form)`
  behaviour so ERP `submit_order` confirmations return to working.
  Also reverted CRM `update_accounts` Edit from `mode: "form" +
  select: "multi"` (which used the bundle's chat-mode submit path
  and emitted a diff-style prompt) back to plain `mode: "prompt"
  + select: "multi"` — the agent now sees a generic "Please
  update… I'd like to change the following fields:" prompt and
  formats the upstream `update_accounts` call itself with the
  `{objects: [...]}` wrapper CRM expects, instead of the diff
  prompt that nudged the agent toward a flat-row payload (which
  reproduced *"Cannot invoke java.util.List.size() because
  'objects' is null"*). The multi-row form code path (Prev/Next
  pages, per-row staged edits, diff prompt on Save all) stays in
  the bundle for operators who want it on tools that accept
  flat-row updates — it just isn't the default for CRM anymore.
- **Nested arrays/objects render as mini-tables in cards and table
  cells (0.1.27).** Previously a field whose value was an array of
  objects (e.g. `items: [{exception: null, payload: {...}, ...}, ...]`
  on `delete_accounts`) collapsed into a single illegible
  `JSON.stringify` line. New `renderValueNode` helper expands one
  nesting level: arrays of plain objects become nested mini-tables,
  single objects become indented key/value blocks, primitive
  arrays become bullet lists. Depth is capped at 2 — deeper
  nesting falls back to a compact JSON pre block so unbounded
  recursion can't blow up the iframe. Primitives in cells keep the
  fast textContent path so the change has no effect on flat tables.
- **Multi-row Edit form with chat-mode submit (0.1.25).** `mode:
  "form"` now supports `select: "multi"`. The inline form opens
  with Prev / Next chrome and a `Row n of N` indicator; per-row
  edits are auto-staged on navigation (no per-row Save button) so
  paging is fluid. On final **Save all** the bundle builds a
  prompt diff (only the *changed* fields per row, with `before →
  after`) and posts it via SEP-1865 `ui/message` — the same chat
  path Delete uses. Same shape works for `select: "single"` with a
  configured `prompt` template, sidestepping a class of upstream
  errors where the inline form's flat-row payload didn't match the
  upstream tool's wrapper shape (e.g. CRM `update_accounts`'s
  `objects: [...]` envelope produced *"Cannot invoke
  java.util.List.size() because 'objects' is null"*). With
  prompt-mode submit the agent re-issues the upstream call with
  whatever shape it expects, and the user sees the change request
  in chat first. CRM's `update_accounts` Edit ships as `select:
  "multi"` by default — both Edit and Delete now share a single
  checkbox column on the table (the dual radio + checkbox columns
  from 0.1.24 only appear when actions disagree on selection mode).
- **Edit button no longer stuck disabled when single + multi actions
  coexist (0.1.24).** Bug fix in `src/bundle/auto.html`'s table
  renderer. Pre-0.1.24, the column-header and row-cell logic used
  `if (wantsMulti) {…} else if (wantsSingle) {…}` — so when a tool
  configured BOTH a single-pick action (`select: "single"`, e.g.
  Edit) AND a multi-pick action (`select: "multi"`, e.g. Delete) on
  the same table, only the checkbox column rendered. No radios
  meant `selection.selectedIdx` was never set, so single-pick
  buttons (Edit) stayed disabled forever. Fix: render BOTH a radio
  column AND a checkbox column when both shapes are configured, so
  `selectedIdx` and `multiIdx` track independently. Also flipped
  CRM's `update_accounts` Edit action from `mode: "prompt"` to
  `mode: "form"` so clicking Edit opens the existing
  `openInlineForm` flow (form pre-filled with the picked row's
  fields, system fields hidden, Submit issues `tools/call` with the
  merged values) — matching the user's stated expectation that Edit
  "show a form with the selected items and let the user change the
  values, item by item." Tools with only one action shape keep
  rendering exactly one selection column.
- **Pre-call confirmation forms (0.1.23).** New top-level
  `formTools[]` array. When the agent issues `tools/call` for a
  listed tool, the policy intercepts the request on the way in and
  replies locally with a synthetic `structuredContent` of
  `{_mcpAppsForm: true, tool, values: <agent args>, fields:
  <declared>}`. The embedded bundle recognises the marker and
  renders a confirmation form pre-filled with the agent's
  arguments, plus any operator-declared optional fields from
  `tools[].formFields[]` (`name`/`label`/`type`/`placeholder`/
  `required`, types `string`/`number`/`boolean`/`json`). Submit
  re-issues `tools/call` with the merged arguments plus an internal
  `_mcpAppsConfirmed: true` marker; the policy strips the marker
  and forwards the confirmed call upstream. Cancel clears the
  iframe without calling the tool. Closes the long-standing gap
  where "Create Order for MULETEST0" gave the user no chance to set
  optional fields before the agent submitted, and adds a final
  confirmation step for destructive tools (Update / Delete /
  Submit) without changing the agent's perception of the call.
  Deny-listed tools are never form tools — the deny list always
  wins. Implements **v0.2 #1**.
- **CSP / formFields are conditional in the Anypoint editor (0.1.23).**
  Removed `default: {}` / `default: []` from `csp:` and the per-tool
  `csp:` / `formFields:` arrays in `gcl.yaml`. Anypoint's policy
  editor used to render four always-visible "Optional" rows
  (`connectDomains`, `resourceDomains`, `frameDomains`,
  `baseUriDomains`) under a permanent CSP block; without defaults,
  the UI collapses these to a single "Add" affordance and the form
  is dramatically less noisy for operators who don't need them.
  Pure UX change — the wire format is unchanged and existing
  configs keep working.
- **Prompt-mode actions (0.1.18–0.1.21).** Actions can declare
  `mode: "prompt"` with a `prompt` template (`${field}` / `${row}` /
  `${rows[].Field}` substitution). On click, the bundle resolves the
  template and renders an in-iframe panel offering **Send to chat**,
  **Copy** and **Dismiss**. Send posts the resolved text to the host
  via the SEP-1865 `ui/message` request (`role: "user"`); the host
  routes it back to the agent as a user turn, so the agent (Claude /
  Goose) re-decides whether to call the underlying tool and the
  action stays inside the conversation chain. Copy writes the prompt
  to the clipboard for hosts that don't support `ui/message`. Closes
  the long-standing gap where Edit / Delete / Order buttons silently
  called the upstream but the agent never saw it happen — pre-0.1.18,
  an Order or Delete could complete without the agent acknowledging
  it. 0.1.18 used invented `ui/notifications/prompt-input` /
  `ui/notifications/intent` names that no host implements; 0.1.19
  replaced them with the spec-correct `ui/message` request gated on
  `hostCapabilities.message`; 0.1.21 drops the capability gate to
  match the official `basic-server-react` reference (Send is always
  enabled, failures surface inline as "use Copy") and lets the user
  pick at click time between sending and copying instead of
  auto-sending. The CRM `update_accounts` / `delete_accounts` actions
  and the ERP `submit_order` / `submit_delivery` actions ship as
  `mode: "prompt"` by default. `mode: "call"` (legacy direct
  `tools/call`) and `mode: "form"` (inline edit form) remain
  available for fire-and-forget actions.

**Tested hosts**

- ✅ **Claude.ai** — full handshake, action buttons, table actions.
- ✅ **Goose** — auto-renders bundles, action buttons fire.
- ✅ **MCP Inspector** — strict `text/html` MIME, strict spec-namespaced
  `_meta` keys, `targetOrigin`-pinned `postMessage`.
- ❌ **ChatGPT** — *not supported*. Tracked under v0.2 below.

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
- **ChatGPT not supported.** ChatGPT does not speak SEP-1865; its
  renderer requires the OpenAI Apps SDK shape — proprietary `_meta`
  keys (`openai/widgetCSP`, `openai/widgetDomain`,
  `openai/outputTemplate`), a `text/html+skybridge` MIME on the
  resource body, and `window.openai.toolOutput` /
  `openai:set_globals` / `window.openai.callTool` on the View side
  rather than `ui/notifications/tool-result` and `tools/call` over
  postMessage. Earlier 0.1.x experiments tried to dual-emit both
  shapes and broke the SEP-1865 hosts every time. Re-introducing
  ChatGPT support without regressing Claude / Goose / Inspector is
  on v0.2.

---

## v0.2 — Schema-driven scaffolding *(planned)*

Goal: reduce config surface for operators by reading what the MCP
server already advertises, and close the highest-leverage UX gaps in
the embedded bundle.

Items below are listed in priority order. **(1)** (pre-call forms)
shipped in 0.1.23 — see the v0.1 Shipped list. The next-priority
items are **(2)–(4)** plus **(6)/(8)**, a low-cost UX bundle that
together make the demos feel finished rather than prototype.

1. **`inputSchema`-driven form fields (next iteration of formTools).**
   0.1.23 ships `formTools[]` with operator-declared `formFields[]`.
   The next step is to read the tool's `inputSchema` from
   `tools/list` and synthesise the form fields automatically —
   required `*`, type-correct inputs, enum dropdowns — so operators
   don't have to repeat what the schema already says. Operator
   `formFields[]` can still override or extend the schema-derived
   set. Cost: small.
2. **Schema-aware cells in the table renderer.** Today every cell is
   text/JSON. Reading `outputSchema` (or `format` hints in the values
   themselves) to render currency, dates, links, enums, and
   booleans-as-checkboxes makes every CRM/ERP demo look 5× more
   polished without any config. Cost: small, all in `auto.html`.
3. **Form validation from `inputSchema` on `mode: form`.** Edit /
   Update forms today are dumb — every key becomes a text input,
   no required-field markers, no type checking. With `inputSchema`
   we get required `*`, type-correct inputs (number / string / date
   / enum dropdown), and an inline error before submit. Cost: small.
4. **Tools/list rewriting (not just deny).** Operators sometimes want
   to rename a tool, hide a parameter, or rewrite the description
   ("Returns accounts" → "Returns accounts in your territory")
   before it reaches the agent. Today the only knob is `denyTools[]`
   — all-or-nothing. A `toolOverrides[]` config covers a real gap.
   Cost: small.
5. **Bundle hosted as a sibling Exchange asset.** Every bundle
   iteration today costs a full policy republish + reapply. Letting
   the bundle live as `mcp-apps-policy-bundle` and be fetched at
   request time would cut iteration from minutes to seconds.
   Cost: medium — needs a fetch path, caching, and a "fall back to
   embedded" guard.
6. **Confirmation dialog before `ui/message` for destructive
   actions.** Delete / Update fire the prompt without a "Are you
   sure? \[N items will be affected\]" affordance. One config flag
   (`actions[].confirm: true | "<text>"`) and a small modal in the
   bundle. Cost: tiny. UX win for production demos.
7. **Telemetry / metrics.** Count appified responses, action clicks
   (per `mode`), prompt-mode sends vs. copies, bundle loads,
   body-cap hits. PDK exposes metrics; we currently expose none.
   Operators running this against real traffic will want this.
   Cost: small.
8. **Error boundary in the bundle.** A `render()` throw today blanks
   the iframe with no clue. A try / catch + error panel
   ("Render failed: …, [Show JSON]") saves hours of debugging.
   Cost: tiny.
9. **Result diff after Edit.** When `mode: form` submits an update
   and the agent responds, the iframe re-renders from scratch.
   Showing "Name: Acme Corp → Acme Inc, Phone unchanged" for a
   few seconds turns Edit from "did anything happen?" into a real
   product. Cost: small.
10. **Streaming-aware bundles.** SSE today is one transform per
    frame; the bundle can't show partial state. For long-running
    tools (a search that streams 500 rows), letting the bundle
    accept incremental `tool-result` updates and append rows
    instead of replacing would be a visible win. Cost: medium —
    needs a small protocol on top of `tool-result`.

Pre-existing v0.2 items, kept:

- **ChatGPT (OpenAI Apps SDK) support.** Re-add the dual emission that
  0.1.12–0.1.15 prototyped, but isolated so it cannot regress SEP-1865
  hosts. Likely shape: a per-host adapter chosen at request time (e.g.
  via the `User-Agent`, an explicit operator opt-in, or a separate
  `ui://…/skybridge/<tool>` URI variant served only when the request
  asks for it). Components needed: dual `_meta` keys on
  `tools/list` / `tools/call` (`openai/widgetCSP`,
  `openai/widgetDomain`, `openai/outputTemplate`),
  `text/html+skybridge` MIME on the matching resource body, and a
  `window.openai` runtime adapter in the embedded bundle that reads
  `toolOutput` / listens for `openai:set_globals` / routes outbound
  calls through `window.openai.callTool`. Acceptance bar: green
  smoke-tests on Claude.ai, Goose, MCP Inspector *and* ChatGPT from
  the same deployment.
- **Schema-driven scaffolding.** Read `tools[].inputSchema` and
  `tools[].outputSchema` to pick the renderer (form for inputs with
  required fields, table for array outputs, card for object outputs)
  and to derive sensible defaults for `argsTemplate`. Subsumed in
  part by **(2)** and **(3)** above; this entry covers the broader
  "auto-pick a renderer from schema" angle.
- **Action discovery from MCP relations.** Once MCP gains a standard
  for "next tool" hints (today this is in flux), wire those into
  `_meta.ui.actions` automatically and treat `tools[].actions[]` as
  overrides.
- **Header-driven renderer negotiation.** Honour a request-side
  `X-MCP-Apps-Renderer: <name>` so an operator can A/B different
  bundles per route without editing config.
- **Per-rule rate caps.** Optional cap on transformations per second
  per tool, to protect the policy itself during incidents.

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
