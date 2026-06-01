# MCP Apps Policy

A custom Flex Gateway / PDK policy that turns any MCP server fronted
by Omni Gateway into an
[MCP Apps](https://modelcontextprotocol.io/extensions/apps/overview)
host surface — without changing the upstream server.

The policy injects the MCP Apps extension metadata
(`io.modelcontextprotocol/ui`, SEP-1865) into discovery and tool
results so MCP-Apps-aware hosts (Claude.ai, VS Code Copilot, Goose,
Postman, MCPJam, Archestra, …) render tools as interactive iframe
apps. Hosts that don't yet support the extension simply ignore the
`_meta.ui` field and behave as before.

> **Status — Public Preview.** MCP Apps (SEP-1865) is itself an
> evolving extension. The policy ships a small, conservative built-in
> bundle and three master switches so operators can roll out
> incrementally.

---

## What it does

| MCP message | What the policy injects |
| --- | --- |
| `tools/list` result | `_meta.ui.resourceUri = "ui://mcp-apps-policy/<toolName>"` on each tool. |
| `resources/list` result | A `ui://` entry per advertised tool. |
| `resources/read` request for `ui://mcp-apps-policy/<tool>` | **Short-circuits the upstream** and returns an embedded HTML5 bundle (`text/html;profile=mcp-app`). |
| `tools/call` result | Copies JSON-bearing `content[0].text` into `result.structuredContent` (preserving any existing one); injects `_meta.ui.actions[]` from the matching tool config. |

Non-JSON-RPC traffic, non-MCP responses, and bodies above
`maxBodyBytes` pass through untouched. The policy never modifies the
request body — its only request-side action is the `resources/read`
short-circuit for its own `ui://` scheme.

---

## Three independently configurable transforms

| Switch | Default | What turning it on does | What turning it off does |
| --- | --- | --- | --- |
| `appifyTools` | `true` | Annotates `tools/list` and adds `ui://` entries to `resources/list`. | Tools come back as the upstream returned them; UI bundles are still served if a host asks for one. |
| `appifyResponses` | `true` | Promotes JSON-bearing `content[0].text` into `result.structuredContent` so the iframe has clean structured data. | `tools/call` results pass through; iframes that depend on `structuredContent` may render less. |
| `appifyActions` | `true` | Injects `_meta.ui.actions[]` from `tools[].actions` / `defaultActions`; the embedded UI renders one button per action. | The iframe shows the rendered result with no buttons. |

Combine the switches with `denyTools[]` to scope the policy
precisely — e.g. annotate only public tools, hide internal admin
tools, and shape responses for everything.

---

## The embedded UI bundle

The policy ships a single self-contained HTML5 bundle (~10 KB of
vanilla JS, no build step, compiled into the WASM artifact via
`include_str!`).

The bundle:

- Sends `ui/initialize` to the host and processes the response.
- Listens for `ui/notifications/tool-result` and re-renders.
- Picks a layout from `_meta.ui.renderer` (or the configured default
  `renderer`):
  - **`table`** for arrays of homogeneous objects.
  - **`card`** for flat objects.
  - **`form`** for objects with editable fields.
  - **`json`** as the safe fallback.
  - **`auto`** picks one of the above based on the result's shape.
- Reads `_meta.ui.actions[]` and renders one button per action.
  Clicking a button issues `tools/call` to the host with the action's
  `tool` and `arguments` (after `${field}` template substitution
  against the current `structuredContent`).
- Reports `ui/notifications/size-changed` so hosts can resize the
  iframe.
- Inherits typography and colors from `hostContext.styles`.

The bundle never speaks to the upstream MCP server directly — every
tool call goes through the host, which preserves the host's auth,
audit, and tool-policy enforcement.

---

## Per-tool overrides

```jsonc
{
  "tools": [
    {
      "name": "get_inventory",
      "renderer": "card",
      "actions": [
        {
          "tool": "create_order",
          "label": "Order",
          "argsTemplate": "{\"sku\":\"${sku}\"}"
        }
      ]
    },
    {
      "name": "list_customers",
      "renderer": "table",
      "actions": [
        { "tool": "get_customer", "label": "Open", "argsTemplate": "{\"id\":\"${id}\"}" }
      ]
    },
    { "name": "secret_admin_tool", "appify": false }
  ],
  "denyTools": ["another_admin_tool"]
}
```

`denyTools` always wins. Per-tool `appify: false` is equivalent in
intent but kept separate so operators can codify "internal admin
tools" once.

---

## Custom bundles

For bespoke component libraries:

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
matching iframe sandbox. Custom bundle names may not shadow built-in
renderer names (`auto`/`json`/`table`/`card`/`form`).

---

## Streamable HTTP / SSE

MCP servers responding with `Content-Type: text/event-stream` (the
Streamable HTTP transport's default) are supported. The policy parses
each `data:` JSON frame, runs the same transform pipeline as a plain
JSON response, and re-emits valid SSE so the client's MCP transport
keeps working. Heartbeat / non-JSON frames pass through.

---

## Operability

| Knob | When to enable |
| --- | --- |
| `previewMode: true` | Adds `x-mcp-apps-preview` JSON describing what the policy did to each response. Use during rollout. |
| `debugHeaders: true` | Adds `x-mcp-apps-method`, `x-mcp-apps-tool`, `x-mcp-apps-action`, `x-mcp-apps-renderer` response headers. Off by default to reduce header churn. |
| `maxBodyBytes` | Cap on the response body the policy will buffer. Bodies larger than this pass through. Default 1 MiB; max 50 MiB. |
| `denyTools[]` | Tools that must never be appified, advertised, or targeted by an action. Takes precedence over everything. |

---

## Limits and gotchas

- **MCP only.** Non-JSON-RPC traffic and bodies whose JSON does not
  parse as a JSON-RPC envelope pass through untouched.
- **One bundle per response.** The auto-rendering bundle picks a
  layout from the live result; it does not compose multiple shapes
  into a single document.
- **No request-body rewriting.** The only request short-circuited is
  `resources/read` for `ui://mcp-apps-policy/...`.
- **Host support varies.** Hosts without MCP Apps support ignore the
  `_meta.ui` field. The list of hosts with native rendering is moving
  fast — see <https://modelcontextprotocol.io/extensions/apps/overview>.
- **Bundles are versioned with the policy.** A bundle change requires
  a policy re-publish today. Bundle hot-reload from a sibling
  Exchange asset is on the roadmap.

---

## License

Copyright 2026 Salesforce, Inc. All rights reserved.
