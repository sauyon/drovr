# Spec: TUI deploy-config picker — browse any config in the project

## Problem

The deployment browser's `V` key is labelled "switch deploy config" but only switches
**versions of the config the deployment is already linked to**, for two independent reasons:

1. **Name-pinned fetch.** `browse_deploy_config.go:76` calls
   `fetchDeployConfigVersions(ctx, c, cfg.BenchmarkProjectName, cfg.Name)` using the
   deployment's *currently linked* config, which resolves to
   `GET /api/v1/benchmark_projects/{project}/deploy_configs?name={name}` — scoped to one
   config name by construction.
2. **Link-gated entry point.** `browse_model.go:1910` rejects with `"not based on a deploy
   config"` when `m.sel.DeployConfigID == nil`, so an unlinked deployment cannot open the
   picker at all.

Every other config in the project — including shared configs, the primary mechanism for
distributing fleet-wide config — is unreachable from `V`.

The server needs no change: `deployConfigService.List` with no name filter already returns
the latest version of every distinct config in the project
(`api-server/services/deploy_config.go:194-201`), shared configs are ordinary rows in that
listing (`reconcileShared`, same file:484-490), and the typed client method already exists
(`api.Client.ListDeployConfigs`, `internal/mcloudcli/api/deploy_config.go:26`).

## Decision: reuse existing pieces, don't rebuild

- **`fieldTree`** (`browse_fieldtree.go:151`) — the browser's existing collapsible tree
  widget (cursor motion, expand/collapse, render) — supplies the picker's UI. It needs one
  new field on `fieldNode`; no new widget.
- **`browse_source_config.go`**'s data plumbing (`loadSrcConfigsCmd` + `latestPerConfig`,
  `fetchDeployConfigVersions`, `ListBenchmarkProjects`, async in-flight/stale-result guards,
  the `count`-omitted-means-`LIMIT 0` pitfall) is reused as-is. Its *presentation* — three
  sequential `huh.Select` prompts — is not: `V` gets a single tree instead.
- `newDeployConfigVersionPicker` and the `huh.Select` step wiring are **not** touched; they
  keep serving the create/edit form and `browse_form_sync.go`.

## Approach

`V` opens a single browsable, collapsible tree with three levels — benchmark project →
config → version — built on `fieldTree`, replacing the current single-config version list.

```
▾ project-a
  ├─ ▸ config-x   (latest v7)  (shared)
  ├─ ▾ config-y   (latest v2)  (current)
  │    ├─ v2  2026-07-01 09:14  bump engine image   (latest) (current)
  │    └─ v1  2026-06-20 11:02  initial
  └─ ▸ config-z   (latest v1)
▸ project-b
```

- `↑`/`↓` move the cursor over visible nodes; `→`/`l` expands, `←`/`h` collapses, `space`
  toggles (existing `fieldTree` methods).
- `enter` on a **version leaf** chooses it and arms the confirm. `enter` on a project or
  config branch only toggles it — a branch is never a selection.
- `esc` cancels the picker with `statusDCCanceled`.

**Why a tree, not a flat list.** A flat list keyed on config row id would force a choice
between dropping version selection (a regression — `V` can pick an older version today) or
loading every version of every config up front. The tree gets both and defers the cost via
lazy loading (below). It also resolves the `Select[uint]` version-number collision the task
flagged: the tree carries its selection payload on the node (`pick`, below), so nothing is
keyed on a bare version number.

### Lazy loading, one level at a time

| Level | Loaded when | Call |
|---|---|---|
| projects | picker opens | `ListBenchmarkProjects` (1 request) |
| configs of a project | that project is first expanded | `ListDeployConfigs(project)` + `latestPerConfig` |
| versions of a config | that config is first expanded | `fetchDeployConfigVersions(project, name)` |

An unexpanded branch holds a single placeholder child (`loading…`, replaced on arrival;
`(none)` when empty) so it renders as expandable before its children are known. Each branch
caches its children after first load; re-collapsing/re-expanding does not refetch.

### On open

- **Linked deployment:** auto-expand its project and its config, cursor on the current
  version — `V`, `↓`, `enter` bumps to a newer version of the current config.
- **Unlinked deployment:** auto-expand the sole project when the org has exactly one project;
  otherwise leave all projects collapsed, cursor on the first.

This means the opening fetch chain (projects → linked project's configs → linked config's
versions) costs no more than the current version-only picker.

### Cross-project adoption

Every project in the org is a tree root, so a deployment can switch to a config in any
project, not just its current one. No transport change: `switchDeployConfigVersion`
(`deploy_config.go:406`) already takes `project` as a parameter, and `confirmState` already
carries `dcProject`; `completeDCSelection` populates it from the chosen leaf's project
instead of the linked config's.

### Out of scope for the tree widget itself

`fieldTree` has no type-to-filter (the old `huh.Select` steps had `.Filtering(true)`).
Adding filtering to `fieldTree` is deferred — see *Follow-ups*.

## Two semantic decisions

### A. Unlinked deployment adopting a config — preserve the deployment's config as overrides

`switchDeployConfigAction` passes `m.deployCfg` as the merge baseline into
`preserveDeploymentDivergences` (`deploy_config.go:448`). For an unlinked deployment that
baseline is `nil` today, and the function returns the target **verbatim** — resetting every
field the deployment has, including e.g. a pinned `engine_spec.image`. This is the same class
of incident the divergence-preservation work (commit `e351fdd69`) was written to prevent.

**Decision:** when the linked baseline is absent, `preserveDeploymentDivergences` performs a
**two-way deep merge** instead of returning the target verbatim: the target config is the
base, the deployment's current configuration is the overlay, and the overlay wins on
conflicting leaves while target-only keys still land. This matches the edit form's existing
fallback (`completeConfigPick`, `browse_source_config.go:445`, using
`overridesFromConfiguration(prior)`) and the semantics of `mergeJSONPtr`
(`browse_form_state.go:730-732`).

**Implementation constraint:** do not implement this by passing an empty map into the
existing `threeWayMerge`. Its "value only current has" branch takes `curVal` **wholesale**
for any key absent from the baseline (`deploy_config.go:479-481`); with an empty baseline
every top-level key the deployment has would replace the target's wholesale, dropping nested
keys the target contributes. The two-way merge must recurse — implement it as a distinct
map-level helper (the map twin of `mergeJSONPtr`), not a variant call into `threeWayMerge`.

This change to `preserveDeploymentDivergences` also fixes the same gap in the existing
`mcloud deployment deploy-config set` CLI path, which shares the function.

### B. Switching across config names — keep the three-way merge

The three-way merge holds fields where the deployment diverged from the config it was
*linked to*. When switching to a different config name (or a different project), that
baseline has no relationship to the target — but the merge is kept anyway: a pinned image
surviving the switch matters more in practice than merge purity, and applying the target
verbatim on a name change would reopen the same unpinning hazard decision A closes. This
applies identically to a cross-project switch — the baseline is the config the deployment was
linked to, wherever it lived. The confirm copy states that overrides carry across.

### Confirm copy

`actionSwitchDeployConfig`'s confirm states, one line each:

- switching **redeploys** the deployment (already true today, unchanged);
- the deployment's **overrides are carried across** (decisions A/B);
- when the chosen config is **shared**: its content is owned by the shared spec, so the next
  `mcloud deploy-config apply` creates a new version from that spec and the config's *latest*
  moves — the deployment stays pinned to the version selected here, because versions are
  immutable rows.

## Interfaces / contracts

### New state on `appModel` (`browse_model.go`, replacing the existing `dc*` block)

```go
dcTree    *fieldTree                     // the picker; non-nil exactly while it is open
dcLinked  *schemasv1.DeployConfigSchema  // the linked config, when any: drives auto-expand,
                                         // cursor placement, and is the merge baseline
                                         // handed to the confirm
dcLoading map[string]bool                // in-flight branch loads, keyed "" (projects) /
                                         // "<project>" / "<project>\x00<config>"
// dcForm, dcVersions, dcCurrent, dcChoice are REPLACED by the tree.
// dcVersLoading is replaced by len(dcLoading) > 0.
```

### `fieldNode` gains a selection payload

`fieldNode` (`browse_fieldtree.go:139-146`) currently carries `label`, `value`, `children`,
`collapsed`, `diff`. Add one optional field:

```go
pick any // non-nil only on picker version leaves; nil on every node the existing trees build
```

Zero value is `nil`, so the config/revision/diff trees are unaffected. The picker sets `pick`
to the `*schemasv1.DeployConfigSchema` on each version leaf; `enter` reads it back. Branch
nodes leave it nil — that nil-ness *is* the "a branch is never a selection" rule; no separate
node-kind enum.

Also add a `current() *fieldNode` accessor on `fieldTree`, returning the node under the
cursor, so the picker does not reach into the unexported `flat`/`cursor` internals.

### New messages

```go
type dcProjectsMsg struct { name string; items []*schemasv1.BenchmarkProjectSchema; err error }
type dcConfigsMsg  struct { name, project string; items []*schemasv1.DeployConfigSchema; err error }
type dcVersionsMsg struct { name, project, config string; items []*schemasv1.DeployConfigSchema; err error }
```

`name` is the **deployment** name on all three, preserving the existing stale-result drop
(`msg.name != m.sel.Name`). `project`/`config` identify which branch the result fills, so a
result arriving after the user collapsed that branch lands on the right node rather than the
cursor's. `dcVersionsMsg` drops the `current uint` field carried by the old design — current
version is known from `dcLinked` and no longer needs to ride on the message.

### Overlay routing

`currentOverlay()` (`browse_model.go:880`) keeps a single arm, now
`case m.dcTree != nil:` returning `overlayHandler{m.updateDCPicker, m.dcPickerView}`.

`autoRefreshPaused()`: the `m.dcVersLoading` arm is replaced by `m.dcTree != nil`, so an
auto-refresh cannot swap `m.sel` out while the picker is open — including while it sits idle
between branch loads, which the old in-flight flag did not cover.

### Entry-point gate

`browse_model.go:1910-1917`: the `DeployConfigID == nil` rejection is dropped. `V` becomes
valid on any detail-screen selection. `helpAdapter.dcLinked` (`browse_model.go:2916`) loses
its `DeployConfigID != nil` condition so the key is advertised on unlinked deployments too.

### Labelling

- Project node: `"<name>"`.
- Config node: `"<name>  (latest v<N>)"`, plus `"  (shared)"` when `Shared` is true, plus
  `"  (current)"` when it is the linked config.
- Version leaf: `"v<N>  <created>  <description>"` — the label
  `newDeployConfigVersionPicker` already builds (`browse_source_config.go:361-362`, reusing
  `trunc`/`sanitizeCell`) — plus `"  (latest)"` on the newest and `"  (current)"` on the
  deployment's current version.
- Panel label (`browse_panels.go:226-228`): `"V: switch version"` → `"V: switch deploy config"`.
- Key help (`browse_model.go:200`) already reads `"switch deploy config"`; no change needed,
  it becomes accurate.

`schemasv1.DeployConfigSchema.Shared` (field 16) is already returned by the list endpoint but
currently surfaced nowhere in the CLI or TUI; this is its first use.

### Transport

Unchanged. `switchDeployConfigVersion` (`deploy_config.go:406`) already takes
`(project, name, version)` via `confirmState`'s `dcProject`/`dcName`/`dcVersion`, and already
sends `deploy_config_id` + full `configuration`. `completeDCSelection` populates
`dcProject`/`dcName` from the chosen config rather than the linked one; no other transport
change.

## Scope boundaries

**In scope**
- `V` reaches every deploy config in the project, shared included.
- `V` works on unlinked deployments.
- Cross-project adoption via the tree's project level.
- `preserveDeploymentDivergences` nil-baseline fix (decision A) — a recursing two-way deep
  merge, added as a new map-level helper — which also fixes the same gap in the existing
  `mcloud deployment deploy-config set` CLI path.
- `pick` payload on `fieldNode` + `current()` accessor on `fieldTree`.
- `(shared)` / `(current)` / `(latest)` markers; confirm copy per above.
- Panel/help label accuracy.
- Tests (below).

**Out of scope**
- Any API-server or schema change — none is required.
- Type-to-filter in the picker. `fieldTree` has no equivalent to `huh.Select`'s
  `.Filtering(true)`; the tree is cursor-navigated only. Deferred — see *Follow-ups*.
- Converting the create/edit form's source picker (`browse_source_config.go`) or the sync
  picker (`browse_form_sync.go`) to the tree. They keep their `huh.Select` steps; only `V`
  changes.
- Warning that editing a shared config gets reverted. Moot: the TUI/CLI has no deploy-config
  write path at all — reads only (`internal/mcloudcli/api/deploy_config.go`); the sole write
  is `mcloud deploy-config apply` (`internal/mcloudcli/cli/deploy_config_cmd.go:86`), the
  shared-apply POST itself. Nothing here edits a config. The one relevant consequence —
  linking to a shared config means the next `deploy-config apply` moves that config's
  *latest* out from under the deployment (the deployment stays pinned to the version it
  selected) — is covered by the confirm copy above.
- Refactoring `browse_source_config.go` and the new `V` flow into one generic picker. They
  differ in presentation (stepped prompts vs. tree), terminal action (reseed a live form vs.
  arm a confirm + PATCH), and entry conditions. Shared data helpers (`latestPerConfig`,
  `findDeployConfigVersion`, `fetchDeployConfigVersions`) are reused as-is, per above.

## Follow-ups (not this change)

- Filtering in `fieldTree` — a shared-widget change (config/revision panels would also
  benefit) with its own blast radius; not part of this change.
- Converting the create/edit source picker to the same tree, once the tree has filtering, so
  the two paths look alike again.

## Testing

Table-driven model tests in the existing style (`browse_deploy_config_test.go`, driving
`appModel.Update` with synthetic messages and a nil client):

1. `V` on an unlinked deployment opens the picker instead of setting
   `"not based on a deploy config"`.
2. `dcConfigsMsg` fills the right project branch with one node per config **name**, and two
   configs' version leaves coexist in one tree carrying distinct `pick` payloads — the
   regression guard for the `Select[uint]` collision.
3. A shared config's node label carries `(shared)`; the linked one carries `(current)`; the
   newest version leaf carries `(latest)`.
4. Auto-expand on open: linked deployment expands its project + config with cursor on the
   current version; unlinked with exactly one project expands that project; unlinked with two
   or more leaves everything collapsed.
5. `enter` on a version leaf arms `confirmState` with **that leaf's** project and config name
   (not the linked config's) — covers the cross-project case.
6. `enter` on a project or config **branch** toggles it and arms **no** confirm (`pick == nil`).
7. Expanding a branch twice issues **one** fetch; children are cached across collapse/expand.
8. Esc clears `dcTree` and sets `statusDCCanceled`.
9. Stale-result drop: a `dcConfigsMsg` for a deployment that is no longer `m.sel` is
   discarded; one for a branch the user has since collapsed fills that branch without moving
   the cursor.
10. `currentOverlay()` routes input to `updateDCPicker` exactly when `dcPickerView` renders.
11. `fieldNode.pick` is nil on every node the existing config/revision/diff tree builders
    produce (guards against the shared-widget change leaking into other panels).

Merge-semantics tests on `preserveDeploymentDivergences` (pure functions, table-driven, no
model):

12. Nil baseline preserves: target config + a deployment carrying a pinned
    `engine_spec.image` and no link ⇒ the pin survives, and a key only the target carries
    still lands (regression guard for the unpinning hazard).
13. Nil baseline recurses: a nested key present in the target but absent from the
    deployment's config is not dropped (guard against the wholesale-replace trap noted in the
    implementation constraint).
14. Non-nil baseline unchanged: existing three-way behaviour is untouched, so the
    cross-config-name case (decision B) keeps carrying divergences.

Plus `mise run go:lint`.

## Files touched

| File | Change |
|---|---|
| `internal/mcloudcli/cli/deployment/browse_deploy_config.go` | Rewritten around the tree: node builders, lazy branch loads, three messages, `updateDCPicker`/`dcPickerView`, `completeDCSelection` from the leaf's `pick`, confirm copy |
| `internal/mcloudcli/cli/deployment/browse_fieldtree.go` | `fieldNode.pick any` + `fieldTree.current()` |
| `internal/mcloudcli/cli/deployment/browse_model.go` | New `dc*` state; `currentOverlay` arm; `V` gate at ~1910; `helpAdapter.dcLinked`; `autoRefreshPaused` |
| `internal/mcloudcli/cli/deployment/browse_panels.go` | Panel label ~226-228 |
| `internal/mcloudcli/cli/deployment/deploy_config.go` | `preserveDeploymentDivergences` nil-baseline branch ⇒ recursing two-way deep merge + new map-level helper |
| `internal/mcloudcli/cli/deployment/browse_deploy_config_test.go` | Tests 1-11 (picker) |
| `internal/mcloudcli/cli/deployment/deploy_config_test.go` | Tests 12-14 (merge semantics) |

## Context

`schemasv1.DeploymentSchema` carries no benchmark-project field (only `DeployConfigID` /
`DeployConfigVersion`, `deployment.go:52`) — this is why the project must come from the
picker tree or from the linked config rather than from the deployment directly.

## Git pointers

- Repo: `/home/sauyon/devel/mcloud/.claude/worktrees/tui-updates` (worktree
  `.drovr/wt/tui-dc-picker`), branch `sauyon/deployment-override-preservation`
- HEAD at spec time: `9bd2dd28a test(e2e): verify override preservation against a live database and cluster`
- Relevant prior work: `e351fdd69 feat(api): re-apply recorded overrides when a deployment switches versions`
