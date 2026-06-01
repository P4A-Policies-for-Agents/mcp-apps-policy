# mcp-apps-policy

A custom policy for **MuleSoft Omni Gateway** ([Flex Gateway](https://docs.mulesoft.com/gateway/latest/) /
PDK) that turns any MCP server fronted by the gateway into an
[MCP Apps](https://modelcontextprotocol.io/extensions/apps/overview)
host surface — without changing the upstream server.

When applied to an API instance whose upstream is an MCP server, the
policy:

- Annotates every advertised tool with the
  `io.modelcontextprotocol/ui` extension metadata
  (`_meta.ui.resourceUri`) so MCP-Apps-aware hosts know to render the
  tool as an iframe app.
- Serves an embedded auto-rendering HTML bundle for those `ui://`
  URIs by short-circuiting `resources/read` — no external CDN, no
  separate hosting, no upstream code changes.
- Shapes `tools/call` results for the UI: copies JSON text content
  into `structuredContent`, then optionally injects suggested
  next-tool actions that the embedded UI renders as buttons.

Hosts that don't support the MCP Apps extension simply ignore the
`_meta.ui` field and behave exactly as they did before.

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
| `tools[]` | array | `[]` | Per-tool overrides — renderer, appify on/off, and actions. |
| `defaultActions[]` | array | `[]` | Actions appended to every tool's button row when `appifyActions` is on. |
| `denyTools[]` | array | `[]` | Tool names that must never be appified, advertised, or targeted by an action. Takes precedence over everything else. |
| `customBundles[]` | array | `[]` | Inline HTML bundles served for specific tools (per-tool `renderer: <name>`). |
| `previewMode` | bool | `false` | Adds `x-mcp-apps-preview` response header with a compact JSON describing what changed. |
| `debugHeaders` | bool | `false` | Adds `x-mcp-apps-method`, `x-mcp-apps-tool`, `x-mcp-apps-action`, `x-mcp-apps-renderer` response headers. |
| `maxBodyBytes` | int | `1048576` | Bodies above this size pass through. Min 1 KiB, max 50 MiB. |

### Per-tool override shape

```jsonc
{
  "name": "get_inventory",
  "renderer": "card",
  "appify": true,
  "actions": [
    {
      "tool": "create_order",
      "label": "Order",
      "argsTemplate": "{\"sku\":\"${sku}\"}"
    }
  ]
}
```

- `renderer` — built-in name (`auto`/`json`/`table`/`card`/`form`) or
  the `name` of a `customBundles[]` entry.
- `appify: false` excludes the tool from app advertisement entirely.
- `actions[].argsTemplate` is a JSON string with `${field}`
  placeholders. At click time the embedded bundle reads each
  placeholder from the *current* result's `structuredContent` and
  substitutes the value before issuing `tools/call`.

### Resource URI scheme

Every appified tool gets a synthesised URI of the form

```
ui://mcp-apps-policy/<encoded-tool-name>
```

Tool names are URL-percent-encoded so unusual characters round-trip
cleanly. The authority `mcp-apps-policy` is fixed — it lets the
policy recognise its own URIs in `resources/read` without consulting
config.

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

# resources/read on a synthesised ui:// URI — answered locally
curl -s http://localhost:8081/mcp \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":2,"method":"resources/read",
       "params":{"uri":"ui://mcp-apps-policy/get_inventory"}}' | jq
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
  --configFile policy-config.json
```

`policy-config.json` in the repo is a fully-worked sample.

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
policy-config.json             # full sample config
ROADMAP.md                     # what's next
```

---

## License

Copyright 2026 Salesforce, Inc. All rights reserved.
