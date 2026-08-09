# Spec: TUI deploy-config picker — browse any config in the project

## Problem

The deployment browser's `V` key is labelled "switch deploy config" but only switches
**versions of the config the deployment is already linked to**. Two gates cause it:

1. **Name-pinned fetch.** `browse_deploy_config.go:76` calls
   `fetchDeployConfigVersions(ctx, c, cfg.BenchmarkProjectName, cfg.Name)` where `cfg` is the
   deployment's *currently linked* config — the listing is scoped to one config name by
   construction.
2. **Link required to enter.** `browse_model.go:1910` rejects with `"not based on a deploy
   config"` when `m.sel.DeployConfigID == nil`, so an unlinked deployment can't open the picker.

Net effect: every shared config, and every other config in the project, is unreachable from `V`.
Shared configs are the acute case — they're how fleet-wide config is distributed, and a
deployment never linked to one can't adopt one from the browser today.

**Server verified not at fault** — no API or schema change is required:
- `deployConfigService.List` (`api-server/services/deploy_config.go:194-201`) with no name filter
  already returns the latest version of every distinct config in the project.
- Shared configs are ordinary `deploy_config` rows materialised by `reconcileShared`
  (`api-server/services/deploy_config.go:484-490`), already in that listing.
- `api.Client.ListDeployConfigs(ctx, project, qs)` (`internal/mcloudcli/api/deploy_config.go:26`)
  already exists and is already used elsewhere.

The gap is narrower than "TUI can't reach shared configs": it's reachable today via `e` (edit) →
source = Deploy config → 3-step picker. What's missing is the **direct, browsable path on the
detail screen** — what `V` claims to be.

## Approach

`V` opens a single browsable, collapsible tree — project → config → version — built on the
browser's existing `fieldTree` widget (`browse_fieldtree.go:151`), replacing the chain of modal
`huh.Select` steps that `browse_source_config.go` uses for the edit-form path. Same three levels
as a stepped picker (so cross-project adoption still falls out of the project level); the
presentation changes from three sequential modal prompts to one navigable view, so you can
compare configs sideways without backing out and restarting.

Reused as-is from `browse_source_config.go`: `loadSrcConfigsCmd` + `latestPerConfig` (listing
configs), `fetchDeployConfigVersions` (listing versions), `ListBenchmarkProjects` (listing
projects), the per-step `*Msg` async in-flight/stale-result pattern, and the count-parameter
pitfall (`srcConfigListCount` / `deployConfigVersionsCount`: an omitted `count` is `LIMIT 0`, not
unlimited — `deploy_config.go:344-349`). Not reused: `newDeployConfigVersionPicker` and the
`huh.Select` wiring — the tree replaces both for `V` only; they keep serving the create/edit form
and `browse_form_sync.go` unchanged.

### The view

```
▾ project-a
  ├─ ▸ config-x   (latest v7)  (shared)
  ├─ ▾ config-y   (latest v2)  (current)
  │    ├─ v2  2026-07-01 09:14  bump engine image   (latest) (current)
  │    └─ v1  2026-06-20 11:02  initial
  └─ ▸ config-z   (latest v1)
▸ project-b
```

- `↑`/`↓` move the cursor (`fieldTree.up`/`down`).
- `→`/`l` expands, `←`/`h` collapses (`fieldTree.setCollapsed`); `space` toggles
  (`fieldTree.toggle`).
- `enter` on a **version leaf** chooses it and arms the confirm. `enter` on a project or config
  branch only toggles it — a branch is never a selection.
- `esc` cancels the picker with `statusDCCanceled`.

### Lazy loading, one level at a time

| Level | Loaded when | Call |
|---|---|---|
| projects | picker opens | `ListBenchmarkProjects` (1 request) |
| configs of a project | that project is first expanded | `ListDeployConfigs(project)` + `latestPerConfig` |
| versions of a config | that config is first expanded | `fetchDeployConfigVersions(project, name)` |

A never-expanded branch holds one placeholder child (`loading…`, replaced on arrival; `(none)`
when empty) so it renders as expandable before its children are known. Each branch caches its
children after first load — re-collapsing/re-expanding does not refetch. This is what keeps the
tree cheaper than a flat "every version of every config" list, which needs all N version listings
up front.

**Rejected alternative — flat list.** Keying on the config row id (`uint`, globally unique — the
clean fix for the `Select[uint]` collision the task flags) forces a choice between dropping
version selection (regression: `V` can pick an older version today) or loading every version of
every config up front. The tree gets both and defers the cost, so it was chosen over the flat
list. The `Select[uint]` collision itself disappears either way, since the tree carries its
selection payload on the node (`pick`, below), not on a bare version number.

### On open

- **Linked deployment:** auto-expand its project and its config, cursor on the **current
  version** — the common case (bump to a newer version of the current config) is `V`, `↓`,
  `enter`.
- **Unlinked deployment:** auto-expand the sole project when the org has exactly one, otherwise
  leave all projects collapsed with the cursor on the first.

This means the opening fetch chain (projects → linked project's configs → linked config's
versions) is the same three requests a stepped picker would have made, so the common path costs
no more than today.

### Cross-project adoption

Every project in the org is a tree root, so a deployment can switch to a config in any project,
not only its current one, and the user can see other projects without committing to one first.
No transport change: `switchDeployConfigVersion` (`deploy_config.go:406`) already takes `project`
as a parameter, and `confirmState` already carries `dcProject`. `completeDCSelection` populates it
from the chosen leaf's project instead of the linked config's.

## Interfaces / contracts

### New state on `appModel` (`browse_model.go`, alongside the existing `dc*` block)

```go
dcTree    *fieldTree                     // the picker; non-nil exactly while it is open
dcLinked  *schemasv1.DeployConfigSchema  // the linked config, when any: drives auto-expand,
                                         // cursor placement, and merge baseline for confirm
dcLoading map[string]bool                // in-flight branch loads, keyed "" (projects) /
                                         // "<project>" / "<project>\x00<config>"
// dcForm, dcVersions, dcCurrent, dcChoice are REPLACED by the tree.
// dcVersLoading is replaced by len(dcLoading) > 0.
```

### `fieldNode` gains a selection payload

`fieldNode` (`browse_fieldtree.go:139-146`) currently has `label`, `value`, `children`,
`collapsed`, `diff` — nothing maps a cursor position to a domain object. New optional field:

```go
pick any // non-nil only on picker version leaves; nil on every node the existing trees build
```

Mirrors how `diff` was added: zero value is `nil`, so config/revision/diff trees are unaffected.
The picker sets `pick` to `*schemasv1.DeployConfigSchema` on version leaves; `enter` reads it
back. Branch nodes leave it `nil` — that nil-ness *is* the "branch is never a selection" rule, no
separate node-kind enum needed.

Also add `current() *fieldNode` on `fieldTree`, returning the node under the cursor, so the
picker doesn't reach into the unexported `flat`/`cursor` internals.

### New messages

```go
type dcProjectsMsg struct { name string; items []*schemasv1.BenchmarkProjectSchema; err error }
type dcConfigsMsg  struct { name, project string; items []*schemasv1.DeployConfigSchema; err error }
type dcVersionsMsg struct { name, project, config string; items []*schemasv1.DeployConfigSchema; err error }
```

`name` is the **deployment** name on all three, preserving the existing stale-result drop
(`msg.name != m.sel.Name`). `project`/`config` identify which branch the result fills, so a
result arriving after the user collapsed that branch fills that branch without moving the cursor.
`dcVersionsMsg` drops the `current uint` field the old design had — current version is now known
from `dcLinked`.

### Overlay routing

`currentOverlay()` (`browse_model.go:880`) keeps a single arm, now `case m.dcTree != nil:`
returning `overlayHandler{m.updateDCPicker, m.dcPickerView}` — simpler than a stepped design's
three-form `dcPickerActive()` with a precedence order, since there is now one form-like object.

`autoRefreshPaused()`: replace the `m.dcVersLoading` arm with `m.dcTree != nil`, so auto-refresh
cannot swap `m.sel` out while the picker is open, including while it sits idle between branch
loads (which the old in-flight flag did not cover).

### Entry-point gate

`browse_model.go:1910-1917` — drop the `DeployConfigID == nil` rejection; `V` becomes valid on
any detail-screen selection. `helpAdapter.dcLinked` (`browse_model.go:2916`) loses its
`DeployConfigID != nil` condition so the key is advertised on unlinked deployments too.

### Labelling

- Project node: `"<name>"`.
- Config node: `"<name>  (latest v<N>)"`, plus `"  (shared)"` when `Shared` is true, plus
  `"  (current)"` when it's the linked config.
- Version leaf: `"v<N>  <created>  <description>"` (reuses the label
  `newDeployConfigVersionPicker` already builds, `browse_source_config.go:361-362`, via
  `trunc`/`sanitizeCell`), plus `"  (latest)"` on the newest and `"  (current)"` on the
  deployment's current version.
- Panel label (`browse_panels.go:226-228`): `"V: switch version"` → `"V: switch deploy config"`.
- Key help (`browse_model.go:200`) already reads `"switch deploy config"` — becomes accurate.

`Shared` (`schemasv1.DeployConfigSchema:16`) is already returned by the list endpoint but
currently surfaced nowhere in the CLI or TUI — this is its first use.

### Transport — unchanged

`switchDeployConfigVersion` (`deploy_config.go:406`) already takes `(project, name, version)` via
`confirmState` (`dcProject`, `dcName`, `dcVersion`) and already sends `deploy_config_id` + full
`configuration`. Cross-config switching needs no transport change — `completeDCSelection`
populates `dcProject`/`dcName` from the chosen config instead of the linked one.

## Two semantic decisions

These change what a switch does to a running deployment, not just how it's picked.

### A. Unlinked deployment adopting a config — preserve current config as overrides

`switchDeployConfigAction` passes `m.deployCfg` as the merge baseline into
`preserveDeploymentDivergences` (`deploy_config.go:448`). For an unlinked deployment that baseline
is `nil` today, and the function returns the target **verbatim** — adopting a config would reset
every field the deployment has, including a pinned `engine_spec.image`. That's the incident class
divergence-preservation was built to prevent (commit `e351fdd69`; the `preserveDeploymentDivergences`
doc comment cites 62 engine replicas rolled onto a floating tag). The edit form already avoids
this via `completeConfigPick`'s fallback to `overridesFromConfiguration(prior)`
(`browse_source_config.go:445`); `V` must match it.

**Rule.** When the linked baseline is absent, `preserveDeploymentDivergences` performs a two-way
deep merge instead of returning the target verbatim: target config is the base, the deployment's
current configuration is the overlay, overlay wins on conflicting leaves, keys only the target
carries still land. This is the semantics of `mergeJSONPtr` in the form
(`browse_form_state.go:730-732`: "deep-merge overlay onto base: new deploy-config fields are
kept, deployment-specific overrides win on conflict").

**Implementation note.** Do not pass an empty map into the existing `threeWayMerge` — its "value
only current has" branch takes `curVal` wholesale for any key absent from the baseline
(`deploy_config.go:479-481`), so an empty-map baseline would replace every top-level key the
deployment has wholesale and drop nested keys the target config contributes. Implement the
two-way merge as a distinct, recursing map-level helper (the map twin of `mergeJSONPtr`), not by
bending `threeWayMerge`.

### B. Switching across config names — keep the three-way merge

The three-way merge holds fields where the deployment diverged from the config it was linked to.
Crossing to a different config, those divergences are computed against a baseline with no
relationship to the target — kept anyway, because a pinned image surviving the switch matters
more in practice than baseline accuracy, and applying verbatim on a name change would reintroduce
the unpinning hazard decision A closes. Applies identically to a cross-project switch — the
baseline is the config the deployment was linked to, wherever it lived. Confirm copy states that
overrides are carried across (below).

### Confirm copy

`actionSwitchDeployConfig`'s confirm states, one line each:
- switching redeploys the deployment (already true today);
- the deployment's overrides are carried across (decision A/B);
- when the chosen config is shared: its content is owned by the shared spec, so the next
  `mcloud deploy-config apply` creates a new version and the config's *latest* moves — the
  deployment stays pinned to the version selected here, since versions are immutable rows.

## Scope boundaries

**In scope**
- `V` reaches every deploy config in the project, shared included.
- `V` works on unlinked deployments.
- Cross-project adoption via the tree's project level.
- Preserving the deployment's configuration when an unlinked deployment adopts a config —
  the `preserveDeploymentDivergences` change, which also fixes the same gap for
  `mcloud deployment deploy-config set`, which shares that function.
- `pick` on `fieldNode` + `current()` on `fieldTree`.
- `(shared)` / `(current)` / `(latest)` markers; confirm copy per above.
- Panel/help label accuracy.
- Tests (below).

**Out of scope**
- Any API-server or schema change.
- Type-to-filter in the picker. `huh.Select` had `.Filtering(true)`; `fieldTree` has no
  equivalent, so the tree is cursor-navigated only in this change (see Follow-ups).
- Converting `browse_source_config.go`'s source picker or `browse_form_sync.go`'s sync picker to
  the tree — they keep their `huh.Select` steps; only `V` changes.
- Warning that editing a shared config gets reverted: moot, since the TUI/CLI have no deploy-config
  write path at all (reads only, `internal/mcloudcli/api/deploy_config.go`; the sole write is
  `mcloud deploy-config apply`, `internal/mcloudcli/cli/deploy_config_cmd.go:86`). The one thing
  worth surfacing — that linking to a shared config means the next `deploy-config apply` moves
  that config's *latest* out from under the deployment's pin — is covered by the confirm's shared
  copy line above.
- Refactoring `browse_source_config.go` and the new `V` flow into one generic picker: they differ
  in presentation (stepped vs. tree), terminal action (reseed a live form vs. arm a confirm +
  PATCH), and entry conditions. Shared data helpers (`latestPerConfig`,
  `findDeployConfigVersion`, `fetchDeployConfigVersions`) are reused as-is.

## Follow-ups (not this change)

- Filtering in `fieldTree` — a shared-widget change (would also benefit the config/revision
  panels) with its own blast radius; not bundled here.
- Converting the create/edit source picker to the same tree, once the tree has filtering, so the
  two paths look alike again.

## Testing

Table-driven model tests in the existing style (`browse_deploy_config_test.go`, driving
`appModel.Update` with synthetic messages and a nil client):

1. `V` on an unlinked deployment opens the picker instead of setting
   `"not based on a deploy config"`.
2. `dcConfigsMsg` fills the right project branch with one node per config name, and two configs'
   version leaves coexist in one tree carrying distinct `pick` payloads (regression guard for the
   old design's `Select[uint]` collision).
3. A shared config's node label carries `(shared)`; the linked one carries `(current)`; the
   newest version leaf carries `(latest)`.
4. Auto-expand on open: linked deployment expands its project + config with cursor on the current
   version; unlinked with one project expands that project; unlinked with two-plus leaves
   everything collapsed.
5. `enter` on a version leaf arms `confirmState` with that leaf's project and config name (not the
   linked config's) — covers the cross-project case.
6. `enter` on a project or config branch toggles it and arms no confirm (`pick == nil`).
7. Expanding a branch twice issues one fetch; children are cached across collapse/expand.
8. Esc clears `dcTree` and sets `statusDCCanceled`.
9. Stale-result drop: a `dcConfigsMsg` for a deployment no longer `m.sel` is discarded; one for a
   branch the user has since collapsed fills that branch without moving the cursor.
10. `currentOverlay()` routes input to `updateDCPicker` exactly when `dcPickerView` renders.
11. `fieldNode.pick` is nil on every node the existing config/revision/diff tree builders produce
    (guards against the shared-widget change leaking into other panels).

Merge-semantics tests on `preserveDeploymentDivergences` (pure functions, table-driven, no model):

12. Nil baseline preserves: target config + a deployment carrying a pinned `engine_spec.image`
    and no link ⇒ the pin survives, and a key only the target carries still lands (regression
    guard for the unpinning hazard).
13. Nil baseline recurses: a nested key present in the target but absent from the deployment's
    config is not dropped (guard against the wholesale-replace trap noted in the implementation
    note).
14. Non-nil baseline is unchanged: existing three-way behaviour holds, so the cross-config-name
    case keeps carrying divergences.

Plus `mise run go:lint`.

## Resolved decisions

| # | Question | Decision |
|---|---|---|
| Q1 | Picker shape | Three-level: project → config → version. Cross-project adoption is consequently in scope. |
| Q5 | Presentation | One browsable collapsible tree, not three sequential `huh.Select` prompts. Same three levels; built on the existing `fieldTree`. Costs type-to-filter (see Follow-ups). |
| Q2 | Project level when it can't be inferred | Auto-expand the sole project when the org has exactly one, otherwise leave projects collapsed — including for linked deployments, whose own project is auto-expanded regardless. |
| Q3 | Unlinked deployment adopting a config | Preserve the deployment's current configuration as overrides (decision A), via a recursing two-way deep merge. |
| Q4 | Switching across config names | Keep the three-way divergence merge against the previously-linked baseline (decision B); confirm copy states it. |

Context: `schemasv1.DeploymentSchema` carries no benchmark-project field (only `DeployConfigID` /
`DeployConfigVersion`, `deployment.go:52`), which is why the project must come from the picker or
from the linked config rather than from the deployment itself.

## Files touched

| File | Change |
|---|---|
| `internal/mcloudcli/cli/deployment/browse_deploy_config.go` | Rewritten around the tree: node builders, lazy branch loads, three messages, `updateDCPicker`/`dcPickerView`, `completeDCSelection` from the leaf's `pick`, confirm copy |
| `internal/mcloudcli/cli/deployment/browse_fieldtree.go` | `fieldNode.pick any` + `fieldTree.current()` |
| `internal/mcloudcli/cli/deployment/browse_model.go` | New `dc*` state; `currentOverlay` arm; `V` gate at ~1910; `helpAdapter.dcLinked`; `autoRefreshPaused` |
| `internal/mcloudcli/cli/deployment/browse_panels.go` | Panel label ~226-228 |
| `internal/mcloudcli/cli/deployment/deploy_config.go` | Q3: `preserveDeploymentDivergences` nil-baseline branch ⇒ recursing two-way deep merge + new map-level helper |
| `internal/mcloudcli/cli/deployment/browse_deploy_config_test.go` | Tests 1-11 (picker) |
| `internal/mcloudcli/cli/deployment/deploy_config_test.go` | Tests 12-14 (merge semantics) |

## Git pointers

- Repo: `/home/sauyon/devel/mcloud/.claude/worktrees/tui-updates` (worktree
  `.drovr/wt/tui-dc-picker`), branch `sauyon/deployment-override-preservation`
- HEAD at spec time: `9bd2dd28a test(e2e): verify override preservation against a live database and cluster`
- Relevant prior work: `e351fdd69 feat(api): re-apply recorded overrides when a deployment switches versions`
