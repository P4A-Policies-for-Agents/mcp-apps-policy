# Tomorrow — Claude.ai still not rendering after v0.1.9

## What we know now (from tonight's parent-console log)

- **Handshake succeeds.** No more `params.appInfo: invalid_type` error. The
  v0.1.8 → v0.1.9 URI cache-bust worked: Claude is loading the new bundle
  with the spec-correct `appInfo` field.
- **Both sides are talking.** Parent console shows
  `Sending message Object` → `Parsed message Object` cycles, so the iframe
  posts and Claude replies.
- **Iframe still doesn't visually render in Claude.ai.** Tool calls
  complete (`[COMPLETION] Completion request succeeded`) but the iframe
  stays blank.
- **`[MCP] tool_approval_gate`** warns every cycle — Claude is putting
  the tool call behind an approval gate. Possible that
  `ui/notifications/tool-result` is being held back until the user
  approves the tool, and that gate is invisible in this UI surface.
- **`oncalltool handler replaced`** — Claude logs that the iframe
  registered an `oncalltool` handler. So the handshake side is fine.
- The `mcp_apps?resource-src=https%3A%2F%2Fassets.claude.ai` URL is
  Claude's own iframe proxy frame, not our bundle. CSP `webrtc`
  warning is noise.

## Hypothesis

Claude isn't pushing `ui/notifications/tool-result` to the iframe —
either because the user hasn't approved the tool call, or because the
host gates `tool-result` on something we're not yet sending (e.g. a
specific `displayMode`, an explicit `available-display-modes` reply
from us, or a non-zero `size-changed` of a *specific* shape).

The bundle handshake renders the placeholder ("Waiting for tool
result…") but never replaces it because no `tool-result` notification
ever arrives on the iframe side.

## Plan — what to do tomorrow

### 1. Re-enable the in-iframe debug overlay

The parent-console log only shows messages going *to* / *from* the
iframe at the top frame. It doesn't show what the iframe itself sees
or whether `tool-result` ever lands inside.

```sh
# already done locally — flip back to true
# policy-config.crm.json + policy-config.erp.json:
#   "previewMode": true
#   "debugHeaders": true
```

Push it:

```sh
# Look up current policy IDs first — they change every re-apply
anypoint-cli-v4 api-mgr policy list 20941724 --environment Sandbox --output json \
  | python3 -c "import sys,json; rows=json.load(sys.stdin); [print(r['ID'],r['Asset ID'],r['Asset Version']) for r in rows]"
anypoint-cli-v4 api-mgr policy list 20941751 --environment Sandbox --output json \
  | python3 -c "import sys,json; rows=json.load(sys.stdin); [print(r['ID'],r['Asset ID'],r['Asset Version']) for r in rows]"

# Remove + re-apply with previewMode=true configs
anypoint-cli-v4 api-mgr policy remove 20941724 <ERP_POLICY_ID> --environment Sandbox
anypoint-cli-v4 api-mgr policy remove 20941751 <CRM_POLICY_ID> --environment Sandbox

anypoint-cli-v4 api-mgr policy apply 20941724 mcp-apps-policy \
  --policyVersion 0.1.9 --groupId f684513f-280e-403a-ac53-80ca62e3de49 \
  --environment Sandbox --upstreamId 23e0ac10-5255-4e52-a5d8-6ffa9bbb355e \
  --configFile policy-config.erp.json

anypoint-cli-v4 api-mgr policy apply 20941751 mcp-apps-policy \
  --policyVersion 0.1.9 --groupId f684513f-280e-403a-ac53-80ca62e3de49 \
  --environment Sandbox --upstreamId d8879c52-d3f3-44b2-8a27-02ac7a9a0ec8 \
  --configFile policy-config.crm.json
```

### 2. In Claude.ai — capture what the iframe actually sees

Open Claude.ai, fresh conversation, run a tool that should render
(e.g. `get_accounts` on CRM). With `previewMode: true`:

- The fixed-position green `[debug]` overlay should attach inside the
  iframe and show every postMessage in/out.
- Look specifically for whether `ui/notifications/tool-result` ever
  appears as `IN` after `OUT ui/notifications/initialized`.
  - If it **does**, the bug is in the bundle's `render()` — log
    `lastResult` shape and confirm the renderer gets called.
  - If it **doesn't**, Claude is not pushing the result. Check
    `tool_approval_gate` — is there an "approve tool call" UI we're
    missing? Check the top-level console for any `tool-result-blocked`
    or `permission-denied` warnings.

### 3. Expand the parent-console `Parsed message` / `Sending message` Objects

In Chrome DevTools, right-click an `Object` in the parent console
(`index-BMYXgLNz.js:2`) and "Store as global variable", then
`console.log(JSON.stringify(temp1))`. We need to see:

- What `method` is in `Parsed message` (likely `ui/initialize` reply
  + `ui/notifications/initialized`).
- Whether any `ui/notifications/tool-result` appears at the parent
  level. If parent sees it but iframe doesn't, it's a postMessage
  origin mismatch (we send to `"*"` so unlikely, but worth ruling out).

### 4. If `tool-result` never arrives — try the available-display-modes path

SEP-1865 has the host advertise `availableDisplayModes` in
`hostContext`. Some hosts gate `tool-result` on the View having
**explicitly opted in to a display mode the host supports**. We
currently send `displayModes: ["inline", "fullscreen"]` in
`appCapabilities`, but never call back to *select* one.

Try (in `src/bundle/auto.html`, after `ui/initialize` resolves):

```js
// Tell the host which mode we want to render in
notify("ui/notifications/display-mode-changed", { displayMode: "inline" });
```

Bump to 0.1.10, ship, re-apply. (Cache-bust is automatic — version
goes into the URI.)

### 5. If still blank — try Claude Desktop instead of Claude.ai

Claude Desktop has the same MCP Apps client but a different sandbox
(no `claudemcpcontent.com` proxy). If it renders there but not on
Claude.ai web, the bug is in the web sandbox, not in our bundle —
file an issue, document the workaround in README troubleshooting,
move on.

### 6. Sanity check — does ChatGPT *actually* render the result?

Tonight's evidence said the handshake completes in ChatGPT (full
hostContext returned), but I never confirmed visually that the table/
card renders. If ChatGPT also stays at "Waiting for tool result…"
then the bug is in our bundle's `render()` path, not Claude-specific.
Test get_accounts in ChatGPT and confirm the table actually appears
before chasing Claude-specific theories.

## Don't get distracted by

- `webrtc` CSP directive warning — just noise, unrelated.
- `faviconV2 ... 404` — unrelated, gstatic noise.
- `Datadog ERR_NAME_NOT_RESOLVED` — Claude.ai's analytics blocked, unrelated.
- `Ignoring message from unknown source MessageEvent` — those are from
  other tabs / extensions, not us.

## Files touched tonight

- `Cargo.toml` — 0.1.9
- `src/mcp/mod.rs` — versioned `ui://` URIs + legacy parser
- `src/bundle/auto.html` — already correct (`appInfo`, `displayModes`)
- `tests/it_*.rs` — updated for versioned shape
- `policy-config.{crm,erp}.json` — flipped to `previewMode: false`
  locally; **need to flip back to `true` and re-apply** before tomorrow
  (or the overlay won't appear).

## Reference

- ERP API instance: `20941724`, upstream `23e0ac10-5255-4e52-a5d8-6ffa9bbb355e`
- CRM API instance: `20941751`, upstream `d8879c52-d3f3-44b2-8a27-02ac7a9a0ec8`
- Group ID: `f684513f-280e-403a-ac53-80ca62e3de49`
- CRM gateway: `https://agent-network-ingress-gw-b2jb0y.1d6nel.usa-e1.cloudhub.io/mcp-apps-crm-demo/mcp`
- ERP gateway: `https://agent-network-ingress-gw-b2jb0y.1d6nel.usa-e1.cloudhub.io/mcp-apps-erp-demo/mcp`
