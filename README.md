# mcp-apps-policy

A custom policy for **MuleSoft Omni Gateway** ([Flex Gateway](https://docs.mulesoft.com/gateway/latest/) /
PDK) that turns any MCP server fronted by the gateway into an
[MCP Apps](https://modelcontextprotocol.io/extensions/apps/overview)
host surface — without changing the upstream server.

## Purpose

Make every MCP server behind the gateway *render-ready* for MCP Apps
hosts (Claude.ai, Goose, MCP Inspector, …) without touching the
upstream. The policy converts standard MCP responses into the shape
MCP Apps hosts expect — `_meta.ui.resourceUri`, an embedded HTML
bundle, `structuredContent`, action buttons — at the gateway edge.

## Goals

- **Zero upstream changes.** Point the gateway at any compliant MCP
  server and apps appear, including servers the operator does not
  own.
- **Single install, fleet-wide.** One asset, one apply per API
  instance; no separate UI hosting, no CDN, no per-server bundle
  pipeline.
- **Backward-safe.** Hosts that don't support the MCP Apps extension
  see exactly the same wire shape they did before — the extra
  `_meta.ui` keys are ignored.
- **Operator-tunable, not LLM-driven.** Layout, action buttons, CSP,
  and pre-call forms come from declarative config — deterministic,
  reviewable, and free of token cost.

## What issue this policy is solving

A normal MCP tool just returns JSON or text. An MCP App bolts a
whole separate app-communication layer on top of standard MCP:

- HTML UI resources
- iframe rendering
- `postMessage` communication
- JSON-RPC interactions across the iframe boundary
- UI lifecycle management
- CSP configuration
- permission handling

👉 **Impact:** bigger codebase, more testing burden, and more things
that can break — for **every** server you'd want to "appify."

## 🎉 Why this policy is a game changer

Because it runs on the Gateway, you don't do any of that heavy
lifting. Point it at any MCP server fronted by the gateway and it
*dynamically* takes whatever the server already offers and converts
it into a rendered MCP App — no upstream code changes, no separate
UI hosting, no per-server protocol plumbing. One gateway-side
install and every server behind it speaks MCP Apps. Hosts that
don't support the extension simply ignore the extra metadata and
behave exactly as before.

## Benefits

- **No per-server engineering.** The bundle, the handshake, the
  CSP, the resource short-circuit — all live in the policy. New MCP
  servers light up the moment the policy is applied.
- **Consistent UX across upstreams.** Every server's tools render
  through the same auto-renderer (table / card / form / json), with
  the same action conventions, the same prompt-mode behaviour, and
  the same theming hooks.
- **Fleet-level rollout and rollback.** Master switches
  (`appifyTools` / `appifyResponses` / `appifyActions`), a deny
  list, and per-tool overrides give operators incremental control —
  enable a few tools, watch the metrics, expand.
- **Pre-call confirmation forms.** `formTools[]` intercepts a
  tool/call and asks the user to confirm or augment the agent's
  arguments before anything reaches the upstream — destructive or
  side-effectful tools become safe by default.
- **Multi-host compatibility.** Spec-namespaced + alias `_meta`
  keys, version-pinned `ui://` URIs, origin-pinned `postMessage`,
  `ui/message` for prompt-mode actions — written for the strict
  hosts (Claude.ai, MCP Inspector) without breaking the relaxed
  ones (Goose, custom MCP clients).
- **Streamable HTTP / SSE compatible.** Same transforms run frame
  by frame on `text/event-stream` responses; heartbeats and
  non-JSON frames pass through unchanged.

---

When applied to an API instance whose upstream is an MCP server, the
policy:

- Annotates every advertised tool with the
  `io.modelcontextprotocol/ui` extension metadata under both the
  spec-namespaced key (`_meta["io.modelcontextprotocol/ui"]`, required
  by Claude.ai) and the relaxed `_meta.ui` alias (Inspector, the
  embedded bundle), so hosts on either side of the namespace debate
  see the `resourceUri` and render the tool as an iframe app.
- Serves an embedded auto-rendering HTML bundle for those `ui://`
  URIs by short-circuiting `resources/read` — no external CDN, no
  separate hosting, no upstream code changes.
- Shapes `tools/call` results for the UI: copies JSON text content
  into `structuredContent`, then optionally injects suggested
  next-tool actions that the embedded UI renders as buttons.

Hosts that don't support the MCP Apps extension simply ignore the
`_meta.ui` field and behave exactly as they did before.

### Tested hosts

| Host | Status |
| --- | --- |
| **Claude.ai** | ✅ Tested |
| **Goose** | ✅ Tested |
| **MCP Inspector** | ✅ Tested |
| **ChatGPT** | ❌ Not supported — see [`ROADMAP.md`](./ROADMAP.md) |

ChatGPT does not speak SEP-1865; its renderer needs the proprietary
OpenAI Apps SDK shape (`openai/widgetCSP`, `openai/widgetDomain`,
`openai/outputTemplate`, `text/html+skybridge` MIME, and the
`window.openai` runtime API). Adding that without breaking the SEP-1865
hosts is on the roadmap.

> **Status — Public Preview.** MCP Apps (SEP-1865) is itself an
> evolving extension; this policy ships a small, conservative built-in
> bundle and three master switches so operators can roll out
> incrementally. See [`ROADMAP.md`](./ROADMAP.md) for what's next.

---

## Table of contents

1. [Why](#why)
2. [How it works](#how-it-works)
3. [Configuration reference](#configuration-reference)
4. [The embedded UI bundle](#the-embedded-ui-bundle)
5. [Custom bundles](#custom-bundles)
6. [Local development](#local-development)
7. [Deploying to Anypoint](#deploying-to-anypoint)
8. [Observability](#observability)
9. [Limits and known gaps](#limits-and-known-gaps)
10. [Troubleshooting](#troubleshooting)
11. [Project layout](#project-layout)

---

## Why

MCP gives agents a uniform way to *call* tools, but their results
still come back as raw JSON or paragraphs of text. Hosts that want to
*show* the result fall back to whatever the model improvises, which:

- Burns tokens on layout decisions the operator already knows how to make.
- Produces inconsistent UI between calls.
- Cannot reuse the operator's existing component library.

The MCP Apps extension solves this by letting servers attach an HTML
bundle to a tool. The host renders the bundle in a sandboxed iframe
and the bundle reads `structuredContent` directly. This policy makes
that contract available to MCP servers that *don't yet emit
`_meta.ui` themselves* — one gateway-side install and every server
behind it speaks MCP Apps.

It is **not** an LLM. It is a transformation policy. If you need an
agent to compose UI dynamically, that still belongs in your agent
runtime; this policy targets the deterministic backbone underneath.

---

## How it works

```
                 ┌─────────────────────────────────────┐
client request ──┤  on_request: only react to MCP      │── pass through
                 │  (JSON-RPC 2.0). Short-circuit       │
                 │  resources/read for ui:// URIs and   │
                 │  serve the embedded bundle locally.  │
                 └────────────┬────────────────────────┘
                              │ RequestData<RequestState>
                              ▼
                 ┌─────────────────────────────────────┐
upstream resp ──▶│  on_response: detect message kind   │
                 │  (tools/list, resources/list,        │
                 │  tools/call), inject _meta.ui /      │
                 │  structuredContent / actions, then   │
                 │  re-emit JSON or SSE on the wire.    │
                 └────────────┬────────────────────────┘
                              ▼
                       MCP-Apps-shaped response
```

Two-phase pipeline (constraint of the PDK):

1. **`on_request`** parses the JSON-RPC request envelope. The only
   request the policy short-circuits is `resources/read` for a
   `ui://mcp-apps-policy/<tool>` URI: the upstream is never called and
   the embedded HTML bundle is returned directly. Every other request
   passes through unchanged.
2. **`on_response`** reads the upstream body (JSON or
   `text/event-stream`), pattern-matches on the JSON-RPC message
   shape, applies the configured transforms, and re-injects the
   rewritten body. Non-MCP traffic, non-JSON bodies, and bodies above
   `maxBodyBytes` are left alone.

The policy only modifies *responses* (and the synthesised
`resources/read` short-circuit). It never touches the request body.

---

## Configuration reference

### Top-level

| Field | Type | Default | Purpose |
| --- | --- | --- | --- |
| `appifyTools` | bool | `true` | (a) Annotate `tools/list` results with `_meta.ui.resourceUri` and add a `ui://` entry per tool to `resources/list`. |
| `appifyResponses` | bool | `true` | (b) Normalise `tools/call` results: copy JSON-bearing `content[0].text` into `result.structuredContent` so the iframe has a clean structured value to render. |
| `appifyActions` | bool | `true` | (c) Inject suggested next-tool actions into `result._meta.ui.actions` based on the matching `tools[]` / `defaultActions` entries. |
| `renderer` | enum | `auto` | Default renderer for the embedded bundle: `auto` \| `json` \| `table` \| `card` \| `form`. |
| `csp` | object | `{}` | Default CSP allowlists emitted as `_meta.ui.csp` on every appified tool. Shape: `{connectDomains[], resourceDomains[], frameDomains[], baseUriDomains[]}`. ChatGPT's app-submission validator rejects templates without a CSP block; Claude.ai ignores the field. |
| `domain` | string | `""` | Default `_meta.ui.domain` for every appified tool. Empty string synthesises `<toolName>.mcp-apps-policy.local` per tool, which satisfies ChatGPT's "unique domain per template" requirement out of the box. |
| `tools[]` | array | `[]` | Per-tool overrides — renderer, appify on/off, actions, plus `csp` / `domain` overrides for hosts that need stable, tool-specific origins. |
| `defaultActions[]` | array | `[]` | Actions appended to every tool's button row when `appifyActions` is on. |
| `denyTools[]` | array | `[]` | Tool names that must never be appified, advertised, or targeted by an action. Takes precedence over everything else. |
| `formTools[]` | array | `[]` | Tool names that should render a **pre-call confirmation form** instead of running immediately. The agent's first `tools/call` is short-circuited with a synthetic result containing the agent's arguments and any operator-declared `formFields[]`. The user reviews / edits / completes the form and submits; the policy strips an internal marker and forwards the confirmed call upstream. Lets the user fill optional fields the agent didn't supply, and gives a final confirmation step for destructive or side-effectful tools. Deny-listed tools are never form tools. |
| `customBundles[]` | array | `[]` | Inline HTML bundles served for specific tools (per-tool `renderer: <name>`). |
| `previewMode` | bool | `false` | Adds an `x-mcp-apps-preview` response header with a compact JSON describing what changed, and injects a `<meta name="x-mcp-debug">` marker into the embedded bundle so the in-iframe debug overlay activates (logs every postMessage on a fixed-position panel). Off in production. |
| `debugHeaders` | bool | `false` | Adds `x-mcp-apps-method`, `x-mcp-apps-tool`, `x-mcp-apps-action`, `x-mcp-apps-renderer` response headers. |
| `maxBodyBytes` | int | `1048576` | Bodies above this size pass through. Min 1 KiB, max 50 MiB. |

### Per-tool override shape

```jsonc
{
  "name": "get_inventory",
  "renderer": "card",
  "appify": true,
  "domain": "inventory.example.com",
  "csp": {
    "connectDomains": ["api.example.com"],
    "resourceDomains": []
  },
  "actions": [
    {
      "tool": "create_order",
      "label": "Order",
      "argsTemplate": "{\"sku\":\"${sku}\"}"
    },
    {
      "tool": "update_accounts",
      "label": "Edit",
      "select": "single",
      "mode": "prompt",
      "prompt": "Please update account ${Id} (${Name}). I want to change: "
    },
    {
      "tool": "delete_accounts",
      "label": "Delete",
      "select": "multi",
      "mode": "prompt",
      "prompt": "Please delete accounts ${rows[].Name} (Ids: ${rows[].Id}). Confirm with me first."
    }
  ]
}
```

- `renderer` — built-in name (`auto`/`json`/`table`/`card`/`form`) or
  the `name` of a `customBundles[]` entry.
- `appify: false` excludes the tool from app advertisement entirely.
- `domain` / `csp` — per-tool overrides for the spec's required
  `_meta.ui.domain` / `_meta.ui.csp` blocks. When omitted, the
  policy falls back to the global defaults; when those are also
  empty, `domain` is synthesised as `<toolName>.mcp-apps-policy.local`
  and `csp` ships as four empty arrays.
- `actions[].select` — row-selection requirement: `none` (default),
  `single` (radio column, button disabled until a row is picked), or
  `multi` (checkbox column, enabled when ≥ 1 row is checked).
- `actions[].mode` — `call` (default; fire `tools/call` immediately
  from the iframe), `form` (open an inline edit form built from the
  selected row, let the user edit, then submit — requires
  `select: single`), or `prompt` (do **not** call the tool from the
  iframe; instead hand a chat-message string back to the host so the
  agent decides whether to call the tool — keeps the conversation
  chain visible, recommended for actions whose outcome the user
  wants to confirm in chat such as Order, Edit, Delete).
- `actions[].prompt` — chat-message template used by `mode: "prompt"`.
  Same `${field}` / `${row}` / `${rows}` / `${rows[].Field}`
  substitution rules as `argsTemplate`. The bundle resolves it at
  click time against the user's pick (or against
  `structuredContent` for `select: none`) and shows it inside an
  in-iframe panel with three buttons: **Send to chat** (post via
  the SEP-1865 `ui/message` request so the prompt appears as a
  user turn and the agent decides whether to call the tool),
  **Copy** (clipboard for manual paste), and **Dismiss**. Following
  the official `basic-server-react` reference, Send is always
  enabled — if the host doesn't implement `ui/message` it
  rejects the request and the button surfaces a *"Not supported —
  use Copy"* fallback.
- `actions[].argsTemplate` is a JSON string with placeholders the
  bundle resolves at click time. With `select: none` the substitution
  happens server-side against `structuredContent` (legacy). With
  `select: single|multi` it happens client-side against the user's
  pick:
  - `${field}` — read a key off the selected row.
  - `${row}` — the entire selected row object.
  - `${rows}` — the array of selected rows (multi only).
  - `${rows[].Field}` — project `Field` across the selected rows
    (multi only).
- `formFields[]` — only honoured when the tool name is also listed
  in the top-level `formTools[]`. Each entry declares an extra field
  to surface on the pre-call form *in addition to* the keys the
  agent already filled in. Useful for optional fields the agent
  often omits. Shape:

  ```jsonc
  {
    "formFields": [
      { "name": "deliveryNotes", "label": "Delivery notes",
        "type": "string", "placeholder": "(optional)" },
      { "name": "priority", "label": "Priority",
        "type": "number", "required": false }
    ]
  }
  ```

  `type` is one of `string` | `number` | `boolean` | `json` (default
  `string`). `required: true` adds an asterisk and blocks submission
  until filled in. The bundle merges agent-supplied values with
  declared fields — the user always sees the agent's choices and
  any extras you declared, in declaration order.

### Pre-call confirmation forms (`formTools[]`)

The default `tools/call` flow runs the upstream tool the moment the
agent decides to call it. For destructive or side-effectful tools
(create / update / delete / order / submit) operators usually want
the user to **confirm and optionally augment** the call first.

Listing a tool name in the top-level `formTools[]` array enables
this flow:

1. Agent issues `tools/call` for the tool.
2. The policy intercepts the request and replies locally with a
   synthetic result whose `structuredContent` is
   `{ _mcpAppsForm: true, tool, values: <agent args>, fields: <declared formFields> }`.
3. The embedded bundle recognises the marker and renders a form
   pre-filled with the agent's values, plus any optional fields you
   declared in `tools[].formFields[]`.
4. On Submit, the bundle re-issues `tools/call` with the merged
   arguments and an internal `_mcpAppsConfirmed: true` marker.
5. The policy strips the marker and forwards the confirmed call to
   the upstream MCP server. The tool runs once, with the user's
   approved arguments.
6. The agent sees a single `tools/call` round-trip — the
   interception is invisible from its perspective.

Cancelling clears the in-iframe state without making the call;
nothing reaches the upstream.

Example: confirm before submitting an order.

```jsonc
{
  "formTools": ["submit_order"],
  "tools": [
    {
      "name": "submit_order",
      "formFields": [
        { "name": "deliveryNotes", "label": "Delivery notes",
          "type": "string", "placeholder": "(optional)" },
        { "name": "expedite", "label": "Expedite shipping",
          "type": "boolean" }
      ]
    }
  ]
}
```

Whatever args the agent supplies (e.g. `{sku, qty}`) appear as
pre-filled inputs; the operator-declared `deliveryNotes` and
`expedite` show up as additional editable fields. Submit fires
`submit_order` once, with the merged payload.

### Resource URI scheme

Every appified tool gets a synthesised URI of the form

```
ui://mcp-apps-policy/v<version>/<encoded-tool-name>
```

Tool names are URL-percent-encoded so unusual characters round-trip
cleanly. The authority `mcp-apps-policy` is fixed — it lets the
policy recognise its own URIs in `resources/read` without consulting
config.

The `v<version>` segment is baked in from the policy's Cargo version
so the URI changes with every release. Hosts that cache the bundle
bytes by URI (notably Claude.ai's `*.claudemcpcontent.com` sandbox
proxy, which is keyed by content hash and otherwise pins the first
bundle it ever fetches) will miss on the new path and refetch the
fresh bundle.

Pre-0.1.9 unversioned URIs (`ui://mcp-apps-policy/<tool>`) are still
recognised on the read path so any cached `_meta.ui.resourceUri`
references from older releases keep working through the transition.

---

## The embedded UI bundle

The bundle is a self-contained HTML5 document (~10 KB of vanilla JS,
no build step). It's compiled into the WASM artifact via
`include_str!`, so there is no separate hosting concern.

What it does:

- Sends the MCP Apps `ui/initialize` handshake to its host.
- Listens for `ui/notifications/tool-result` and re-renders.
- Picks a layout from `_meta.ui.renderer` (or the configured default
  `renderer`):
  - **`table`** for arrays of homogeneous objects.
  - **`card`** for flat objects (key/value grid).
  - **`form`** for objects with editable fields (no submit by default;
    wire one with an action).
  - **`json`** as the safe fallback.
  - **`auto`** picks one of the above based on the result's shape.
- Reads `_meta.ui.actions[]` and renders one button per action; on
  click, posts a `tools/call` to the host with the action's `tool`
  and rendered `arguments` (after `${field}` substitution).
- Reports `ui/notifications/size-changed` so hosts can resize the
  iframe.
- Themes from `hostContext.styles` so the bundle inherits the host
  app's typography and colors.

The bundle never speaks to the upstream MCP server directly — every
call goes through the host, which preserves the host's auth, audit,
and tool-policy enforcement.

---

## Custom bundles

Operators with bespoke component libraries can register inline HTML:

```jsonc
{
  "customBundles": [
    {
      "name": "inventory-card",
      "html": "<!DOCTYPE html>...full HTML5 document...",
      "csp": {
        "connectDomains": ["api.salesforce.com"],
        "resourceDomains": ["cdn.salesforce.com"]
      }
    }
  ],
  "tools": [
    { "name": "get_inventory", "renderer": "inventory-card" }
  ]
}
```

The `html` string is served verbatim as `text/html;profile=mcp-app`.
The `csp` block is emitted as `_meta.ui.csp` so the host can set up a
matching iframe sandbox. The bundle must implement the MCP Apps
postMessage protocol — see
<https://apps.extensions.modelcontextprotocol.io/api/>.

A custom bundle name may not shadow a built-in renderer name.

---

## Local development

```sh
make setup                 # install cargo-anypoint + fetch deps
make test                  # cargo test (unit + integration via pdk-unit)
make build                 # WASM + GCL artifacts
make run                   # build + bring up Flex Gateway + httpbin in Docker
make publish               # publish a *development* asset version to Exchange
make release               # promote to a *release* asset version
make upload-docs           # upload definition/home.md as the Exchange home page
```

`make run` patches `playground/config/api.yaml` with the freshly
built policy reference, copies the implementation/definition GCL into
`playground/config/custom-policies/`, and starts a Flex Gateway
listening on `http://localhost:8081`.

Edit `playground/config/api.yaml`'s `services.upstream.address` to
point at a real MCP server (e.g. one of the demo CRM/ERP MCP
endpoints) and re-run `make run` — the policy will start annotating
that server's `tools/list` and shaping its `tools/call` results.

```sh
# tools/list — every tool comes back with _meta.ui.resourceUri
curl -s http://localhost:8081/mcp \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/list"}' | jq

# resources/read on a synthesised ui:// URI — answered locally.
# Use whatever URI was advertised in the previous tools/list response;
# the path includes the policy version (e.g. `v0.1.9`).
curl -s http://localhost:8081/mcp \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":2,"method":"resources/read",
       "params":{"uri":"ui://mcp-apps-policy/v0.1.27/get_inventory"}}' | jq
```

---

## Deploying to Anypoint

```sh
make release
```

This builds the WASM + GCL, then publishes:

- `<group>:mcp-apps-policy:<version>` (definition asset).
- `<group>:mcp-apps-policy-impl:<version>` (implementation asset, depends on the definition).

### Apply to an API instance

The policy injects on the **outbound** path (responses), so it needs
an upstream id, not an inbound apply:

```sh
# Discover the upstream id
anypoint-cli-v4 api-mgr upstream list <API_INSTANCE_ID> --environment Sandbox

# Apply
anypoint-cli-v4 api-mgr policy apply <API_INSTANCE_ID> mcp-apps-policy \
  --policyVersion <VERSION> \
  --groupId <GROUP_ID> \
  --environment Sandbox \
  --upstreamId <UPSTREAM_ID> \
  --configFile policy-config.<server>.json
```

Each MCP server gets its own config file because the `tools[]`
overrides only apply to tools that actually exist on that upstream —
mixing them produces phantom resources. The repo includes two
fully-worked samples:

- `policy-config.crm.json` — CRM tools (`get_accounts`,
  `create_Accounts`, `update_accounts`, `delete_accounts`).
- `policy-config.erp.json` — ERP tools (`get_inventory`,
  `submit_order`, `submit_delivery`).

Adapt one per upstream and pass the right file with `--configFile`.

### Verify

```sh
anypoint-cli-v4 api-mgr policy list <API_INSTANCE_ID> --environment Sandbox
```

Look for `mcp-apps-policy` in the listing with your config rendered as
YAML below it.

### Smoke-test the gateway

```sh
# tools/list — should now include _meta.ui.resourceUri on each tool
curl -s -X POST <gateway-endpoint> \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/list"}' | jq

# tools/call — result should carry structuredContent and (if configured) _meta.ui.actions
curl -s -X POST <gateway-endpoint> \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":2,"method":"tools/call",
       "params":{"name":"get_inventory","arguments":{"sku":"A1"}}}' | jq
```

Allow ~30 seconds after `policy apply` for the Flex Gateway to pick
up the new assignment.

---

## Observability

| Signal | Source | When |
| --- | --- | --- |
| `x-mcp-apps-preview` header | response | `previewMode: true` — JSON `{action, tool}` describing what the policy did |
| `x-mcp-apps-method` header | response | `debugHeaders: true` — the JSON-RPC method that was processed |
| `x-mcp-apps-tool` header | response | `debugHeaders: true` — tool name (when applicable) |
| `x-mcp-apps-action` header | response | `debugHeaders: true` — `annotated` / `injected` / `untouched` / etc. |
| `x-mcp-apps-renderer` header | response | `debugHeaders: true` — chosen renderer for the response |
| Policy logs | Flex Gateway logs | Always (level: `info` for normal operation, `warn` for fallbacks) |

For production: leave `debugHeaders` off and use `previewMode` only
during rollout.

---

## Limits and known gaps

- **MCP only.** Non-JSON-RPC traffic and bodies whose JSON does not
  parse as a JSON-RPC envelope pass through untouched.
- **SSE transform is per-event.** Each `data:` frame is transformed
  independently; cross-event state is not maintained. For Streamable
  HTTP this matches the MCP wire format. Heartbeats / non-JSON frames
  pass through.
- **One bundle per response.** The auto-rendering bundle picks a
  layout from the live result; it does not compose multiple shapes
  into a single document.
- **Body size cap.** Bodies above `maxBodyBytes` (default 1 MiB) are
  passed through. Increase up to 50 MiB if you control the upstream.
- **No request-body rewriting.** Requests are inspected (method,
  path, headers) and the only request short-circuited is
  `resources/read` for our `ui://` scheme.
- **Host support varies.** Hosts without MCP Apps support ignore the
  `_meta.ui` field and behave as before. The list of hosts with
  native rendering is moving fast — check
  <https://modelcontextprotocol.io/extensions/apps/overview>.

---

## Troubleshooting

| Symptom | Likely cause | Fix |
| --- | --- | --- |
| Tools come back without `_meta.ui` | Upstream is not MCP / response is not JSON-RPC, or `appifyTools: false`, or every tool is on the `denyTools` list | Confirm the upstream is MCP and `appifyTools` is on; check `denyTools`. |
| Iframe never renders | Host doesn't speak MCP Apps yet | Try a host that does (Claude.ai, VS Code Copilot, Goose, MCPJam). The wire format is still correct — non-supporting hosts just ignore `_meta.ui`. |
| Iframe loads but stays empty / "Waiting for tool result…" | Host gates `tool-result` pushes on a successful `ui/initialize`, or on a non-zero `size-changed` report. Turn on `previewMode: true` to enable the in-iframe debug overlay (`<meta name="x-mcp-debug">`); it logs every postMessage and surfaces handshake errors. |
| Iframe renders an *old* bundle / `ui/initialize` is rejected with a field name (e.g. `appInfo`) the current source no longer uses | Host's sandbox proxy cached the previous bundle by URI. Bump the policy version — the `v<version>` segment in `ui://mcp-apps-policy/v<version>/<tool>` will change and the proxy will refetch. |
| Claude.ai logs *"Tool X has no UI resource (no ui/resourceUri in tool._meta)"* even though `_meta.ui.resourceUri` is set | Strict SEP-1865 hosts only read `_meta["io.modelcontextprotocol/ui"]` (the spec-namespaced key) and ignore the `_meta.ui` alias. As of 0.1.10 the policy emits both. Upgrade to ≥ 0.1.10 and re-apply. |
| ChatGPT shows *"Widget CSP is not set for this template. A CSP is required for app submission"* and/or *"Widget domain is not set for this template. A unique domain is required for app submission"* | The host requires `_meta.ui.csp` and `_meta.ui.domain` per template. As of 0.1.11 the policy emits both — empty allowlists for CSP, and a synthesised `<toolName>.mcp-apps-policy.local` domain when not explicitly set. Upgrade to ≥ 0.1.11 and re-apply, or set `csp` / `domain` (global or per-tool) for stable values. |
| Edit / Delete buttons appear in a table but do nothing useful | Pre-0.1.11 actions resolved `${field}` against the *top-level* `structuredContent` and never the picked row, so list-shaped tools (e.g. `get_accounts`) shipped empty `Id`s. As of 0.1.11, actions can declare `select: single` / `select: multi` and the bundle renders a radio/checkbox column; `${field}`, `${row}`, `${rows}`, and `${rows[].Field}` resolve against the user's pick at click time. `mode: form` opens an inline edit form for the selected row and submits the edits as `tools/call` arguments. |
| Edit / Delete / Order succeeds but the agent never knows it happened (the action runs silently from the iframe and the conversation chain is broken) | Pre-0.1.18 every action fired `tools/call` directly from the iframe — the upstream got the call but the agent (Claude / Goose) never saw it. As of 0.1.19, `mode: "prompt"` posts the resolved message to the host via the SEP-1865 `ui/message` request (`role: "user"`); the host routes it back as a user turn and the agent re-decides whether to call the tool. Apps gate the call on `hostCapabilities.message` from `ui/initialize`; when the host doesn't advertise it (older Claude.ai builds, some Goose versions) the bundle shows the resolved prompt in an in-iframe panel with a **Copy** button as a manual fallback. (0.1.18 used invented `ui/notifications/prompt-input` / `ui/notifications/intent` names that no host implements; 0.1.19 replaces them with the spec-correct `ui/message`.) |
| MCP Inspector iframe never sees `tool-result` | Pre-0.1.11 the bundle posted with `targetOrigin: "*"` which Inspector's strict CSP can drop. As of 0.1.11 the bundle pins `targetOrigin` to the host's origin captured from the first inbound message. |
| Action button arguments are empty | `argsTemplate` references a `${field}` not present in `structuredContent` | Inspect the tool's `result.structuredContent` and align the template. |
| `resources/read` for a `ui://` URI hits the upstream | Policy load order / wrong policy ref | Check `anypoint-cli-v4 api-mgr policy list` shows `mcp-apps-policy` applied. |
| `Cannot process message because this session hasn't been initialized yet` | Upstream uses Streamable HTTP and requires an `initialize` handshake first | Issue an MCP `initialize`, capture the `mcp-session-id` response header, send it on subsequent calls. |
| Custom bundle ignored | Bundle `name` collides with a built-in renderer (`auto`/`json`/`table`/`card`/`form`) | Rename the bundle. |

---

## Project layout

```
src/
  lib.rs                       # entrypoint + filter wiring (proxy-wasm)
  config.rs                    # typed PolicyConfig + validation
  mcp/                         # JSON-RPC dispatch (tools/list, tools/call,
                               #   resources/list, resources/read) + SSE
  bundle/                      # embedded auto-rendering HTML bundle
  generated/config.rs          # cargo-anypoint config-gen
definition/
  gcl.yaml                     # Anypoint asset GCL (config schema)
  home.md                      # Exchange-facing landing page
playground/                    # local Flex docker-compose harness
tests/
  it_tools_list.rs             # tools/list annotation tests
  it_tools_call.rs             # tools/call shaping + actions tests
  it_resources_read.rs         # ui:// short-circuit tests
  it_sse.rs                    # Streamable HTTP / SSE tests
  common/                      # shared backend harness
policy-config.crm.json         # sample config for the CRM upstream
policy-config.erp.json         # sample config for the ERP upstream
ROADMAP.md                     # what's next
```

---

## License

Copyright 2026 Salesforce, Inc. All rights reserved.
