# Backlog — `mcp-apps-policy`

A short, actionable checklist for picking the work back up.

Last updated: 2026-06-01.

---

## Where things stand

- `cargo test` is green: 30 unit + 10 integration tests, all
  passing.
- WASM release build succeeds with the embedded HTML bundle compiled
  in via `include_str!`.
- Repo is laid out for an Anypoint release (Cargo.toml metadata,
  Makefile, `definition/gcl.yaml`, playground harness, sample
  `policy-config.json`, README/ROADMAP/home.md).
- The policy has **not yet been published** to Exchange and has
  **not yet been applied** to any API instance.

## Today's open thread

The task that was in flight at end-of-day is **publishing + applying
to API instances `20941724` and `20941751`**. The Anypoint CLI is
already configured locally (`anypoint-cli-v4 ...`).

---

## Resume checklist (in order)

### 1. Smoke-test the build locally

```sh
cd /Users/amir.khan/Documents/cursor-workspaces/mcp-apps-policy
make test                                    # should be green
make build                                   # generates WASM + GCL artifacts
```

If `cargo anypoint config-gen` fails on `definition/gcl.yaml`,
re-check the schema field types — the hand-authored
`src/generated/config.rs` needs to match what `config-gen` would
emit.

### 2. Publish a development version (optional, recommended first)

```sh
make publish
```

This pushes a *development* asset version to Exchange. Use it to
validate the GCL renders correctly in the Anypoint UI before
promoting.

### 3. Release the policy to Exchange

```sh
make release
make upload-docs                              # uploads definition/home.md
```

Captures the released `<VERSION>` from CLI output (or read it from
`Cargo.toml`'s `version = "..."`).

If the asset-id validator rejects `mcp-apps-policy` (digit-after-letter
rule that bit `a2ui-policy`), try `mcp-apps-pdk-policy` or similar
hyphen-separated form. The Cargo metadata fields to update if so:

```
[package.metadata.anypoint]
definition_asset_id     = "..."
implementation_asset_id = "...-impl"
```

### 4. Apply to API instances 20941724 and 20941751

For each API instance:

```sh
# Discover the upstream id (the policy injects on outbound)
anypoint-cli-v4 api-mgr upstream list 20941724 --environment Sandbox
anypoint-cli-v4 api-mgr upstream list 20941751 --environment Sandbox

# Apply
anypoint-cli-v4 api-mgr policy apply 20941724 mcp-apps-policy \
  --policyVersion <VERSION> \
  --groupId f684513f-280e-403a-ac53-80ca62e3de49 \
  --environment Sandbox \
  --upstreamId <UPSTREAM_ID> \
  --configFile policy-config.json

anypoint-cli-v4 api-mgr policy apply 20941751 mcp-apps-policy \
  --policyVersion <VERSION> \
  --groupId f684513f-280e-403a-ac53-80ca62e3de49 \
  --environment Sandbox \
  --upstreamId <UPSTREAM_ID> \
  --configFile policy-config.json
```

Verify:

```sh
anypoint-cli-v4 api-mgr policy list 20941724 --environment Sandbox
anypoint-cli-v4 api-mgr policy list 20941751 --environment Sandbox
```

Expect to see `mcp-apps-policy` listed with the YAML-rendered config
underneath.

### 5. Smoke-test against the live MCP servers

The two MCP servers behind those instances are:

- https://agent-network-ingress-gw-b2jb0y.1d6nel.usa-e1.cloudhub.io/a2ui-crm-demo/mcp
- https://agent-network-ingress-gw-b2jb0y.1d6nel.usa-e1.cloudhub.io/a2ui-erp-demo/mcp

Wait ~30 s after `policy apply` for Flex to pick up the assignment.

```sh
# tools/list — every tool should now have _meta.ui.resourceUri
curl -s -X POST <one of the URLs above> \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/list"}' | jq

# resources/read on a synthesised ui:// URI — answered locally with HTML
curl -s -X POST <one of the URLs above> \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":2,"method":"resources/read",
       "params":{"uri":"ui://mcp-apps-policy/<some-tool-name>"}}' | jq
```

If `resources/read` returns the upstream's response instead of the
embedded bundle, the policy isn't engaged — re-check the
`policy list` output.

### 6. Tail logs if anything looks off

```sh
anypoint-cli-v4 api-mgr policy enable 20941724 mcp-apps-policy   # if disabled
# (Flex Gateway logs are wherever your runtime sends them — Anypoint
# Monitoring → Runtime → APIs → 20941724 → Logs)
```

`previewMode: true` in `policy-config.json` makes diagnosis easier
(adds an `x-mcp-apps-preview` header to every response). Turn it off
once verified.

---

## Things to watch for

- **`mcp-session-id`.** MCP servers that use Streamable HTTP require
  an `initialize` handshake first; subsequent calls must carry the
  `mcp-session-id` response header. The policy doesn't manage this
  for you — clients still need to do the dance.
- **CSP in custom bundles.** If you switch a tool to a custom bundle,
  set `customBundles[].csp.connectDomains` to whatever the bundle's
  JS calls; the host enforces this in the iframe sandbox.
- **Bundle size in the WASM.** The auto bundle is ~10 KB. If a
  custom bundle balloons the WASM past Flex's policy size limit,
  move that bundle to the planned v0.2 sideload path instead.

---

## After today

If publish + apply lands cleanly, the next reasonable thread is the
**v0.2 schema-driven scaffolding** work in `ROADMAP.md` — read
`tools[].outputSchema` and pick the renderer automatically instead
of asking operators to configure one per tool.

---

## Key paths

- Source: `src/lib.rs`, `src/mcp/`, `src/bundle/`, `src/config.rs`
- Schema: `definition/gcl.yaml`
- Sample config: `policy-config.json`
- Local dev: `playground/docker-compose.yaml`,
  `playground/config/api.yaml`
- Tests: `tests/it_*.rs`, `tests/common/mod.rs`
- Group id (Anypoint): `f684513f-280e-403a-ac53-80ca62e3de49`
