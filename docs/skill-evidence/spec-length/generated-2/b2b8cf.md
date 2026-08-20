# Spec: TUI deploy-config picker — browse any config in the project

## Problem

The deployment browser's `V` key is labelled "switch deploy config" but only switches
**versions of the config the deployment is already linked to**, for two reasons:

1. `browse_deploy_config.go:76` fetches versions name-pinned to the linked config
   (`fetchDeployConfigVersions(ctx, c, cfg.BenchmarkProjectName, cfg.Name)`).
2. `browse_model.go:1910` rejects the key entirely when `m.sel.DeployConfigID == nil`
   ("not based on a deploy config"), so an unlinked deployment cannot open the picker.

Every other config in the project — including shared configs, which are how fleet-wide
config is distributed — is unreachable from `V`. No server change is needed:
`deployConfigService.List` with no name filter already returns the latest version of every
config in the project, including shared ones (`api-server/services/deploy_config.go:194-201`,
`484-490`), and `api.Client.ListDeployConfigs` already exposes it
(`internal/mcloudcli/api/deploy_config.go:26`).

## Approach

`V` opens a single browsable, collapsible **tree** with three levels — benchmark project →
config → version — built on the browser's existing `fieldTree` widget
(`browse_fieldtree.go:151`). A tree, not a chain of `huh.Select` steps, because browsing
sideways ("what else is in this project? what other versions does that config have?") is the
point of the feature, and a stepped picker cannot compare two configs without backing out and
restarting.

```
▾ project-a
  ├─ ▸ config-x   (latest v7)  (shared)
  ├─ ▾ config-y   (latest v2)  (current)
  │    ├─ v2  2026-07-01 09:14  bump engine image   (latest) (current)
  │    └─ v1  2026-06-20 11:02  initial
  └─ ▸ config-z   (latest v1)
▸ project-b
```

- `↑`/`↓` move the cursor over visible nodes (`fieldTree.up`/`down`).
- `→`/`l` expands, `←`/`h` collapses (`fieldTree.setCollapsed`); `space` toggles
  (`fieldTree.toggle`).
- `enter` on a **version leaf** chooses it and arms the confirm. `enter` on a project or
  config branch toggles it — a branch is never a selection.
- `esc` cancels the picker with `statusDCCanceled`.

### Lazy loading, one level at a time

Expanding a branch is what triggers fetching it, so a large org costs nothing until you go
looking:

| Level | Loaded when | Call |
|---|---|---|
| projects | picker opens | `ListBenchmarkProjects` (1 request) |
| configs of a project | that project is first expanded | `ListDeployConfigs(project)` + `latestPerConfig` |
| versions of a config | that config is first expanded | `fetchDeployConfigVersions(project, name)` |

A branch that has never been expanded holds a single placeholder child (`loading…`, replaced
on arrival; `(none)` when the listing is empty), so it renders as expandable before its
children are known. Each branch caches its children after the first load; re-collapsing and
re-expanding does not refetch. This is what makes the tree strictly cheaper than a flat list
of every version of every config, which would need all N version listings up front.

### On open

- **Linked deployment:** auto-expand its project and its config, cursor on the current
  version, so the common case (bump to a newer version of the current config) is `V`, `↓`,
  `enter`.
- **Unlinked deployment:** auto-expand the sole project when the org has exactly one,
  otherwise leave all projects collapsed with the cursor on the first.

This means the opening fetch chain — projects → the linked project's configs → the linked
config's versions — is the same three requests a stepped picker would have made, so the
common path costs no more than today.

### Cross-project adoption

Every project in the org is a root of the tree, so a deployment can switch to a config in any
project, not just its own — a direct consequence of the three-level shape. This needs no
transport change: `switchDeployConfigVersion` (`deploy_config.go:406`) already takes `project`
as a parameter, and `confirmState` already carries `dcProject`; `completeDCSelection`
populates it from the chosen leaf's project instead of the linked config's.

## Interfaces / contracts

### New state on `appModel` (`browse_model.go`, alongside the existing `dc*` block)

```go
dcTree    *fieldTree                     // the picker; non-nil exactly while it is open
dcLinked  *schemasv1.DeployConfigSchema  // the linked config, when any: drives the auto-expand
                                         // and cursor placement, and is the merge baseline
                                         // handed to the confirm
dcLoading map[string]bool                // in-flight branch loads, keyed "" (projects) /
                                         // "<project>" / "<project>\x00<config>"
// dcForm, dcVersions, dcCurrent, dcChoice are REPLACED by the tree.
// dcVersLoading is replaced by len(dcLoading) > 0.
```

### `fieldNode` gains a selection payload

`fieldNode` (`browse_fieldtree.go:139-146`) gets one new optional field:

```go
pick any // non-nil only on picker version leaves; nil on every node the existing trees build
```

Zero value is `nil`, so the config/revision/diff trees are unaffected (mirrors how `diff` was
added). The picker sets `pick` to the `*schemasv1.DeployConfigSchema` on each version leaf;
`enter` reads it back. Branch nodes leave it nil — that nil-ness *is* the "a branch is never a
selection" rule, so no separate node-kind enum is needed.

Also add `current() *fieldNode` on `fieldTree`, returning the node under the cursor, so the
picker does not reach into the unexported `flat`/`cursor` internals.

### New messages

```go
type dcProjectsMsg struct { name string; items []*schemasv1.BenchmarkProjectSchema; err error }
type dcConfigsMsg  struct { name, project string; items []*schemasv1.DeployConfigSchema; err error }
type dcVersionsMsg struct { name, project, config string; items []*schemasv1.DeployConfigSchema; err error }
```

`name` is the deployment name on all three, preserving the existing stale-result drop
(`msg.name != m.sel.Name`). `project`/`config` identify which branch the result fills, so a
result arriving after the user collapsed that branch lands on the right node rather than the
cursor's. `dcVersionsMsg` drops the old `current uint` field — the current version is now
known from `dcLinked`.

### Overlay routing

`currentOverlay()` (`browse_model.go:880`) keeps a single arm, now `case m.dcTree != nil:`,
returning `overlayHandler{m.updateDCPicker, m.dcPickerView}`.

`autoRefreshPaused()`: replace the `m.dcVersLoading` arm with `m.dcTree != nil`, so an
auto-refresh cannot swap `m.sel` out while the picker is open — including while it sits open
and idle between branch loads, which the old in-flight flag did not cover.

### Entry-point gate

`browse_model.go:1910-1917`: drop the `DeployConfigID == nil` rejection. `V` becomes valid on
any detail-screen selection. `helpAdapter.dcLinked` (`browse_model.go:2916`) loses its
`DeployConfigID != nil` condition so the key is advertised on unlinked deployments too.

### Labelling

- Project node: `"<name>"`.
- Config node: `"<name>  (latest v<N>)"`, plus `"  (shared)"` when `Shared` is true, plus
  `"  (current)"` when it is the linked config.
- Version leaf: `"v<N>  <created>  <description>"` (reusing the label
  `newDeployConfigVersionPicker` already builds, `browse_source_config.go:361-362`, via
  `trunc`/`sanitizeCell`), plus `"  (latest)"` on the newest and `"  (current)"` on the
  deployment's current version.
- Panel label (`browse_panels.go:226-228`): `"V: switch version"` → `"V: switch deploy config"`.
- Key help (`browse_model.go:200`) already reads `"switch deploy config"` — now accurate.

`Shared` (`schemasv1.DeployConfigSchema:16`) is already returned by the list endpoint but
currently surfaced nowhere in the CLI or TUI; this is its first use.

### Transport — unchanged

`switchDeployConfigVersion` (`deploy_config.go:406`) already takes `(project, name, version)`
via `confirmState` (`dcProject`, `dcName`, `dcVersion`) and already sends `deploy_config_id` +
full `configuration`. Cross-config switching needs no transport change —
`completeDCSelection` populates `dcProject`/`dcName` from the chosen config rather than the
linked one.

## Decisions on switch semantics

These change what a switch does to a running deployment, not just what the picker looks like.

### A. Unlinked deployment adopting a config — preserve its configuration as overrides

`switchDeployConfigAction` passes `m.deployCfg` as the merge baseline into
`preserveDeploymentDivergences` (`deploy_config.go:448`). For an unlinked deployment that
baseline is `nil` today, and the function returns the target **verbatim** — resetting every
field the deployment has, including a pinned `engine_spec.image`. That is the exact incident
class the divergence-preservation work (commit `e351fdd69`) exists to prevent, and the edit
form already avoids it via `overridesFromConfiguration(prior)`
(`browse_source_config.go:445`); `V` must match.

**Decision:** when the linked baseline is absent, `preserveDeploymentDivergences` performs a
**two-way deep merge** instead of returning the target verbatim: target config is the base,
the deployment's current configuration is the overlay, overlay wins on conflicting leaves,
and keys only the target carries still land. This is the map-level twin of the semantics
`mergeJSONPtr` already documents (`browse_form_state.go:730-732`).

**Implementation constraint:** this must be a distinct map-level helper, not
`threeWayMerge` called with an empty-map baseline. `threeWayMerge`'s "value only current has"
branch takes `curVal` wholesale for any key absent from the baseline
(`deploy_config.go:479-481`); with an empty baseline every top-level key the deployment has
would replace the target's wholesale, and nested target keys would be dropped. The new helper
must recurse.

### B. Switching across config names — keep the three-way merge

The three-way merge holds fields where the deployment diverged from the config it was linked
to; when crossing to a different config, that baseline has no relationship to the target.

**Decision:** keep the three-way merge anyway. A pinned image surviving the switch is the
behaviour that matters most in practice, and applying the target verbatim on a name change
would reintroduce the unpinning hazard decision A closes. Applies identically to a
cross-project switch — the baseline is the config the deployment was linked to, wherever it
lived. Confirm copy states that overrides are carried across (see *Confirm copy*).

### Picker shape and project-level auto-expand

- **Three levels — project → config → version.** Cross-project adoption follows directly:
  every project in the org is a tree root, so any config in any project is reachable.
- **Project auto-expand:** the sole project auto-expands when the org has exactly one;
  otherwise all projects start collapsed, cursor on the first — including for a linked
  deployment, whose own project is auto-expanded regardless of this rule (its project/config
  auto-expand is the *On open* behavior above, not the org-size rule).

  Reason this needs stating: `schemasv1.DeploymentSchema` carries no benchmark-project field
  (only `DeployConfigID`/`DeployConfigVersion`, `deployment.go:52`), so for an unlinked
  deployment there is no deployment-side signal for which project to default to — hence the
  org-cardinality rule rather than inference from the deployment.

### Confirm copy

`actionSwitchDeployConfig`'s confirm states, one line each:

- switching redeploys the deployment (already true today);
- the deployment's overrides are carried across (decision A/B);
- when the chosen config is shared: its content is owned by the shared spec, so the next
  `mcloud deploy-config apply` creates a new version from that spec and the config's *latest*
  moves — the deployment stays pinned to the version selected here, because versions are
  immutable rows.

## Scope boundaries

**In scope**
- `V` reaches every deploy config in the project, shared included.
- `V` works on unlinked deployments.
- Cross-project adoption via the tree's project level.
- Preserving the deployment's configuration when an unlinked deployment adopts a config
  (decision A) — a change to `preserveDeploymentDivergences`, which also fixes the same gap
  for the existing `mcloud deployment deploy-config set` path that shares it.
- `pick` payload on `fieldNode` + a `current()` accessor on `fieldTree`.
- `(shared)` / `(current)` / `(latest)` markers; confirm copy per *Confirm copy* above.
- Panel/help label accuracy.
- Tests (below).

**Out of scope**
- Any API-server or schema change — not needed (see *Problem*).
- Type-to-filter in the picker. `huh.Select` had `.Filtering(true)`; `fieldTree` has no
  equivalent, so the tree is cursor-navigated only for this change. Tracked as a follow-up on
  the widget itself, not here, because it is a shared-widget change with its own blast radius
  (the config/revision panels use the same widget).
- Converting the create/edit form's source picker (`browse_source_config.go`) or the sync
  picker (`browse_form_sync.go`) to the tree. They differ from `V` in presentation (stepped
  prompts vs. tree), terminal action (reseed a live form vs. arm a confirm + PATCH), and entry
  conditions, so they keep their `huh.Select` steps; only `V` changes. Shared *data* helpers
  (`latestPerConfig`, `findDeployConfigVersion`, `fetchDeployConfigVersions`) are reused as-is.
- A warning that editing a shared config gets reverted: there is no deploy-config write path
  in the TUI or CLI at all (reads only, `internal/mcloudcli/api/deploy_config.go`; the sole
  write is `mcloud deploy-config apply`, `internal/mcloudcli/cli/deploy_config_cmd.go:86`), so
  there is nothing to warn about. The adjacent, real consequence — linking to a shared config
  means the next `deploy-config apply` moves that config's *latest* out from under the
  deployment — is covered by the *Confirm copy* shared-config line.

## Testing

Table-driven model tests in the existing style (`browse_deploy_config_test.go`, driving
`appModel.Update` with synthetic messages and a nil client):

1. `V` on an unlinked deployment opens the picker instead of setting
   `"not based on a deploy config"`.
2. `dcConfigsMsg` fills the right project branch with one node per config name, and two
   configs' version leaves coexist in one tree carrying distinct `pick` payloads (regression
   guard for the old `Select[uint]` collision).
3. A shared config's node label carries `(shared)`; the linked one carries `(current)`; the
   newest version leaf carries `(latest)`.
4. Auto-expand on open: linked deployment expands its project + config with the cursor on the
   current version; unlinked with exactly one project expands that project; unlinked with two
   or more leaves everything collapsed.
5. `enter` on a version leaf arms `confirmState` with that leaf's project and config name (not
   the linked config's) — covers the cross-project case.
6. `enter` on a project or config branch toggles it and arms no confirm (`pick == nil`).
7. Expanding a branch twice issues one fetch; children are cached across collapse/expand.
8. Esc clears `dcTree` and sets `statusDCCanceled`.
9. Stale-result drop: a `dcConfigsMsg` for a deployment that is no longer `m.sel` is discarded;
   one for a branch the user has since collapsed fills that branch without moving the cursor.
10. `currentOverlay()` routes input to `updateDCPicker` exactly when `dcPickerView` renders.
11. `fieldNode.pick` is nil on every node the existing config/revision/diff tree builders
    produce (guards the shared-widget change against leaking into other panels).

Merge-semantics tests on `preserveDeploymentDivergences` (pure functions, table-driven, no
model):

12. Nil baseline preserves: target config + a deployment carrying a pinned
    `engine_spec.image` and no link ⇒ the pin survives, and a key only the target carries
    still lands. Regression guard for the unpinning hazard.
13. Nil baseline recurses: a nested key present in the target but absent from the deployment's
    config is not dropped — guards against the wholesale-replace trap called out in decision A.
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

## Git pointers

- Repo: `/home/sauyon/devel/mcloud/.claude/worktrees/tui-updates` (worktree
  `.drovr/wt/tui-dc-picker`), branch `sauyon/deployment-override-preservation`
- HEAD at spec time: `9bd2dd28a test(e2e): verify override preservation against a live database and cluster`
- Relevant prior work: `e351fdd69 feat(api): re-apply recorded overrides when a deployment switches versions`
