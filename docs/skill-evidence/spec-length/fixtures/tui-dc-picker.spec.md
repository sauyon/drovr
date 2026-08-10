# Spec: TUI deploy-config picker — browse any config in the project

## Problem

The deployment browser's `V` key is labelled "switch deploy config" but can only switch
**versions of the config the deployment is already linked to**. Two independent gates cause it:

1. **The fetch is name-pinned.** `browse_deploy_config.go:76` calls
   `fetchDeployConfigVersions(ctx, c, cfg.BenchmarkProjectName, cfg.Name)` where `cfg` is the
   deployment's *currently linked* config. That resolves to
   `GET /api/v1/benchmark_projects/{project}/deploy_configs?name={name}`, so the listing is
   scoped to one config name by construction.
2. **The entry point requires a link.** `browse_model.go:1910` short-circuits with
   `"not based on a deploy config"` when `m.sel.DeployConfigID == nil`, so an unlinked
   deployment cannot open the picker at all.

Net effect: **every shared config, and every other config in the project, is unreachable from
`V`.** Shared configs are the acute case — they are how fleet-wide config is distributed, and
a deployment that was never linked to one can never adopt one from the browser.

### The server is not the problem — verified

- `deployConfigService.List` (`api-server/services/deploy_config.go:194-201`) with **no** name
  filter returns the latest version of every distinct config in the project
  (`SELECT DISTINCT ON (name) * ... ORDER BY name, version DESC`).
- Shared configs are **ordinary `deploy_config` rows** materialised into the target
  org + project by `reconcileShared` (`api-server/services/deploy_config.go:484-490`,
  `Shared: true`), so they are already in that listing.
- The typed client method already exists and is already used elsewhere:
  `api.Client.ListDeployConfigs(ctx, project, qs)` (`internal/mcloudcli/api/deploy_config.go:26`).

No API or server change is required.

### What already exists that we should reuse, not rebuild

Two existing pieces carry most of this feature.

**`fieldTree` (`browse_fieldtree.go:151`)** — the browser's interactive collapsible tree, already
backing the config, revision, and diff panels. It supplies the entire widget: `up`/`down` cursor
motion, `toggle`/`setCollapsed` expand-collapse, `reflatten` visible-node recomputation, and a
`render` that draws real `├──`/`└──` connectors with the cursor row highlighted and a `▸`
affordance on collapsed branches. The picker needs one new field on `fieldNode` (below) and no
new widget.

**`browse_source_config.go`** — the create/edit form's project → config → version drill-down.
Its *presentation* (three sequential `huh.Select` prompts) is not what we want here, but its
data plumbing is exactly right and is reused wholesale:

| Concern | Existing solution |
|---|---|
| Listing all configs in a project | `loadSrcConfigsCmd` → `ListDeployConfigs` + `latestPerConfig` (`browse_source_config.go:190-217`) |
| Listing a config's versions | `fetchDeployConfigVersions` (`deploy_config.go:363`) |
| Listing projects | `ListBenchmarkProjects` (`internal/mcloudcli/api/benchmark_project.go:15`) |
| Async in-flight guards / stale-result drops | per-step `*Msg` handlers with identity checks |
| Count-parameter pitfall | `srcConfigListCount` / `deployConfigVersionsCount` — an omitted `count` arrives as `LIMIT 0`, **not** unlimited (`deploy_config.go:344-349`) |

Not reused: `newDeployConfigVersionPicker` and the `huh.Select` step wiring — the tree replaces
both for `V`. They stay where they are, still serving the create/edit form and
`browse_form_sync.go`.

The user-visible gap is therefore narrower than "the TUI cannot reach shared configs": the TUI
*can* reach them via `e` (edit) → source = Deploy config → 3-step picker. What is missing is the
**direct, browsable path on the detail screen** — which is exactly what `V` claims to be.

## Approach

`V` opens a **single browsable, collapsible tree** with three levels — benchmark project →
config → version — built on the browser's existing `fieldTree` widget
(`browse_fieldtree.go:151`), not on a chain of modal `huh.Select` steps.

This keeps Q1's three-level answer intact: the levels are the same, and cross-project adoption
still falls out of the project level. What changes is the **presentation** — one navigable view
you can browse up and down, expanding and collapsing as you go, instead of three sequential
modal prompts you commit to one at a time. Browsing sideways ("what else is in this project?
what other versions does that config have?") is the point of the feature, and a stepped picker
makes exactly that awkward: you cannot compare two configs without backing out and starting over.

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

- `↑`/`↓` move the cursor over visible nodes (`fieldTree.up`/`down`).
- `→` / `l` expands, `←` / `h` collapses (`fieldTree.setCollapsed`); `space` toggles
  (`fieldTree.toggle`).
- `enter` on a **version leaf** chooses it and arms the confirm. `enter` on a project or config
  branch just toggles it — a branch is never a selection.
- `esc` cancels the whole picker with `statusDCCanceled`.

### Lazy loading, one level at a time

Expanding is what triggers fetching, so a large org costs nothing until you go looking:

| Level | Loaded when | Call |
|---|---|---|
| projects | picker opens | `ListBenchmarkProjects` (1 request) |
| configs of a project | that project is first expanded | `ListDeployConfigs(project)` + `latestPerConfig` |
| versions of a config | that config is first expanded | `fetchDeployConfigVersions(project, name)` |

A branch that has never been expanded holds a single placeholder child (`loading…`, replaced on
arrival; `(none)` when the listing is empty) so it renders as expandable before its children are
known. Each branch caches its children after the first load — re-collapsing and re-expanding does
not refetch. This is what makes the tree strictly cheaper than the "flat list of every version of
every config" alternative, which needs all N version listings up front.

### On open

- **Linked deployment:** auto-expand its project and its config, and place the cursor on the
  **current version**, so the common case (bump to a newer version of the config I am on) is
  `V`, `↓`, `enter` with everything else browsable from there.
- **Unlinked deployment:** auto-expand the sole project when the org has exactly one (Q2),
  otherwise leave all projects collapsed with the cursor on the first.

Auto-expanding on open means the opening fetches chain: projects → the linked project's configs →
the linked config's versions. Those are the same three requests the stepped design would have
made, so the common path costs no more than before.

### Why not a flat list

A flat list would have to be keyed on the config **row id** (`uint`, globally unique — that is
the clean fix for the collision the task flags), but it forces a choice between dropping version
selection (regression: `V` can pick an older version today) and loading every version of every
config up front. The tree gets both, and defers the cost.

The `Select[uint]` collision the task names disappears entirely: the tree carries its selection
payload on the node (see `pick` below), so nothing is keyed on a bare version number.

### Cost of this shape

The `huh.Select` steps had `.Filtering(true)`; `fieldTree` has **no type-to-filter**. For an org
with many configs, browsing is by cursor alone. Adding filtering to `fieldTree` is **out of scope
here** — flagged in *Follow-ups* rather than silently dropped.

### Cross-project adoption

Because every project in the org is a root of the tree, a deployment can be switched to a config
in any of them, not just its current one — and unlike a stepped picker, the user can see the
other projects sitting there without committing to one first. This needs **no transport change**:
`switchDeployConfigVersion` (`deploy_config.go:406`) already takes `project` as a parameter, and
`confirmState` already carries `dcProject`. `completeDCSelection` populates it from the chosen
leaf's project instead of the linked config's.

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

`fieldNode` (`browse_fieldtree.go:139-146`) carries `label`, `value`, `children`, `collapsed`,
`diff` — nothing that maps a cursor position back to a domain object. Add one optional field:

```go
pick any // non-nil only on picker version leaves; nil on every node the existing trees build
```

This mirrors how `diff` was added: the zero value is `nil`, so the config, revision, and diff
trees are unaffected. The picker sets `pick` to the `*schemasv1.DeployConfigSchema` on each
version leaf and `enter` reads it back. Branch nodes leave it nil, which *is* the "a branch is
never a selection" rule — no separate node-kind enum needed.

Also add a `current() *fieldNode` accessor on `fieldTree` returning the node under the cursor, so
the picker does not reach into the unexported `flat`/`cursor` internals.

### New messages

```go
type dcProjectsMsg struct { name string; items []*schemasv1.BenchmarkProjectSchema; err error }
type dcConfigsMsg  struct { name, project string; items []*schemasv1.DeployConfigSchema; err error }
type dcVersionsMsg struct { name, project, config string; items []*schemasv1.DeployConfigSchema; err error }
```

`name` is the **deployment** name on all three, preserving the existing stale-result drop
(`msg.name != m.sel.Name`). `project`/`config` identify which branch the result fills, so a
result arriving after the user collapsed that branch lands on the right node rather than the
cursor's.

`dcVersionsMsg` loses its `current uint` field — the current version is known from `dcLinked`
and no longer needs to ride on the message.

### Overlay routing

`currentOverlay()` (`browse_model.go:880`) keeps a **single** arm, now `case m.dcTree != nil:`
returning `overlayHandler{m.updateDCPicker, m.dcPickerView}`. This is simpler than the stepped
alternative, which would have needed a three-form `dcPickerActive()` with a precedence order:
there is one form-like object, so the input-routing/render pairing invariant that file documents
holds trivially.

`autoRefreshPaused()`: replace the `m.dcVersLoading` arm with `m.dcTree != nil`, so an auto
-refresh cannot swap `m.sel` out while the picker is open — including while it sits open and
idle between branch loads, which the old in-flight flag did not cover.

### Entry-point gate

`browse_model.go:1910-1917` — drop the `DeployConfigID == nil` rejection. `V` becomes valid on
any detail-screen selection. `helpAdapter.dcLinked` (`browse_model.go:2916`) loses its
`DeployConfigID != nil` condition so the key is advertised on unlinked deployments too.

### Labelling

- Project node: `"<name>"`.
- Config node: `"<name>  (latest v<N>)"`, plus `"  (shared)"` when `Shared` is true, plus
  `"  (current)"` when it is the linked config.
- Version leaf: `"v<N>  <created>  <description>"` (the label
  `newDeployConfigVersionPicker` already builds, `browse_source_config.go:361-362`, reusing
  `trunc`/`sanitizeCell`), plus `"  (latest)"` on the newest and `"  (current)"` on the
  deployment's current version.
- Panel label (`browse_panels.go:226-228`): `"V: switch version"` → `"V: switch deploy config"`.
- Key help (`browse_model.go:200`) already reads `"switch deploy config"` — now accurate.

`Shared` is on `schemasv1.DeployConfigSchema:16` and already returned by the list endpoint; it
is currently surfaced **nowhere** in the CLI or TUI, so this is its first use.

### Transport — unchanged

`switchDeployConfigVersion` (`deploy_config.go:406`) already takes `(project, name, version)`
via the confirm state (`dcProject`, `dcName`, `dcVersion` on `confirmState`) and already sends
`deploy_config_id` + full `configuration`. Cross-config switching needs **no transport change** —
`completeDCSelection` just has to populate `dcProject`/`dcName` from the chosen config rather
than from the linked one.

## Two semantic decisions this exposes — both now settled

These are not cosmetic; they change what a switch does to a running deployment.

### A. Unlinked deployment adopting a config — **preserve the deployment's config as overrides**

`switchDeployConfigAction` passes `m.deployCfg` as the merge baseline into
`preserveDeploymentDivergences` (`deploy_config.go:448`). When the deployment is unlinked that
baseline is `nil`, and the function returns the target **verbatim** — so adopting a config would
**reset every field the deployment has**, including a pinned `engine_spec.image`. That is the
exact class of incident the divergence-preservation work was written to prevent
(commit `e351fdd69`, and the `preserveDeploymentDivergences` doc comment citing 62 engine
replicas rolled onto a floating tag).

The edit form already avoids this: `completeConfigPick` (`browse_source_config.go:445`) falls
back to `overridesFromConfiguration(prior)` when there is no linked baseline. `V` must match it.

**Rule.** When the linked baseline is absent, `preserveDeploymentDivergences` must stop returning
the target verbatim and instead perform a **two-way deep merge**: the target config is the base,
the deployment's current configuration is the overlay, and the **overlay wins on conflicting
leaves** while keys only the target carries still land. That is exactly the semantics of the
form's `mergeJSONPtr`, whose doc comment states it directly — *"deep-merge overlay onto base: new
deploy-config fields are kept, deployment-specific overrides win on conflict"*
(`browse_form_state.go:730-732`).

**Implementation note — do not just pass an empty map into the existing merge.** Reusing
`threeWayMerge` with an empty-map baseline gives the wrong answer: its "value only current has"
branch takes `curVal` **wholesale** for any key absent from the baseline
(`deploy_config.go:479-481`), so with an empty baseline every top-level key the deployment has
would replace the target's wholesale, and nested keys the config contributes would be dropped.
The two-way merge must recurse. Implement it as a distinct map-level helper (the map twin of
`mergeJSONPtr`) rather than bending `threeWayMerge`.

### B. Switching across config *names* — **keep the three-way merge**

The three-way merge holds fields where the deployment diverged **from the config it was linked
to**. Crossing to a different config, those divergences are computed against a baseline that has
no relationship to the target. The merge is kept anyway: a pinned image surviving the switch is
the behaviour that matters most in practice, and applying verbatim on a name change would
reintroduce the exact unpinning hazard decision A exists to close. The confirm copy must say
that overrides are being carried across (see *Confirm copy* below).

This applies to a cross-**project** switch identically — the baseline is the config the
deployment was linked to, wherever it lived.

### Confirm copy

The `actionSwitchDeployConfig` confirm should state, in one line each:

- that switching **redeploys** the deployment (already true today);
- that the deployment's **overrides are carried across** (decision A/B);
- when the chosen config is **shared**: that its content is owned by the shared spec, so the next
  `mcloud deploy-config apply` creates a new version from that spec and the config's *latest*
  moves — the deployment stays pinned to the version selected here, because versions are
  immutable rows.

## Scope boundaries

**In scope**
- `V` reaches every deploy config in the project, shared included.
- `V` works on unlinked deployments.
- Cross-**project** adoption, via the tree's project level (a consequence of Q1's three-level
  answer).
- Preserving the deployment's configuration when an unlinked deployment adopts a config (Q3) —
  a change to `preserveDeploymentDivergences`, which also fixes the same gap for the existing
  `mcloud deployment deploy-config set` path that shares it.
- `pick` payload on `fieldNode` + a `current()` accessor on `fieldTree`.
- `(shared)` / `(current)` / `(latest)` markers; confirm copy per *Confirm copy* above.
- Panel/help label accuracy.
- Tests (below).

**Out of scope**
- Any API-server or schema change.
- **Type-to-filter in the picker.** `huh.Select` had `.Filtering(true)`; `fieldTree` has no
  equivalent, so the tree is cursor-navigated only. See *Follow-ups*.
- Converting the create/edit form's source picker (`browse_source_config.go`) or the sync picker
  (`browse_form_sync.go`) to the tree. They keep their `huh.Select` steps; only `V` changes.
- **Warning that editing a shared config gets reverted** — resolved by fact, not by design:
  *the TUI and CLI have no deploy-config write path at all.* Reads only
  (`internal/mcloudcli/api/deploy_config.go`); the sole write is
  `mcloud deploy-config apply` (`internal/mcloudcli/cli/deploy_config_cmd.go:86`), the
  shared-apply POST itself. There is nothing to warn about because nothing here edits a config.
  What *is* worth one line of copy is the consequence of **linking to** a shared config: the next
  `deploy-config apply` creates a new version from the shared spec, so the config's *latest*
  moves out from under you (your deployment stays pinned to the version it selected —
  versions are immutable rows). Proposed as a `Description` line on the confirm, not a blocker.
- Refactoring `browse_source_config.go` and the new `V` flow into one generic picker. They differ
  in presentation (stepped prompts vs. tree), in terminal action (reseed a live form vs. arm a
  confirm + PATCH), and in entry conditions. Shared *data* helpers (`latestPerConfig`,
  `findDeployConfigVersion`, `fetchDeployConfigVersions`) are reused as-is.

## Follow-ups (not this change)

- **Filtering in `fieldTree`.** The tree has no type-to-filter, so an org with many configs is
  cursor-navigated only. Worth adding to the widget itself, where the config/revision panels
  would benefit too — but it is a shared-widget change with its own blast radius and does not
  belong in this one.
- **Converting the create/edit source picker to the same tree**, once the tree has filtering, so
  the two paths look alike again.

## Testing

Table-driven model tests in the existing style (`browse_deploy_config_test.go`, driving
`appModel.Update` with synthetic messages and a nil client):

1. `V` on an unlinked deployment opens the picker instead of setting
   `"not based on a deploy config"`.
2. `dcConfigsMsg` fills the right project branch with one node per config **name**, and two
   configs' version leaves coexist in one tree carrying distinct `pick` payloads — the
   regression guard for the `Select[uint]` collision the old design had.
3. A shared config's node label carries `(shared)`; the linked one carries `(current)`; the
   newest version leaf carries `(latest)`.
4. Auto-expand on open: linked deployment expands its project + config with the cursor on the
   current version; unlinked with one project expands that project; unlinked with two or more
   leaves everything collapsed.
5. `enter` on a version leaf arms `confirmState` with **that leaf's** project and config name
   (not the linked config's) — covering the cross-project case, where the chosen project differs
   from the linked config's.
6. `enter` on a project or config **branch** toggles it and arms **no** confirm (`pick == nil`).
7. Expanding a branch twice issues **one** fetch; children are cached across collapse/expand.
8. Esc clears `dcTree` and sets `statusDCCanceled`.
9. Stale-result drop: a `dcConfigsMsg` for a deployment that is no longer `m.sel` is discarded;
   one for a branch the user has since collapsed fills that branch without moving the cursor.
10. `currentOverlay()` routes input to `updateDCPicker` exactly when `dcPickerView` renders.
11. `fieldNode.pick` is nil on every node the existing config/revision/diff tree builders
    produce (guards the shared-widget change against leaking into other panels).

Merge-semantics tests on `preserveDeploymentDivergences` (pure functions, table-driven, no model):

12. **Nil baseline preserves (Q3).** Target config + a deployment carrying a pinned
    `engine_spec.image` and no link ⇒ the pin survives, and a key only the target carries
    still lands. This is the regression guard for the unpinning hazard.
13. **Nil baseline recurses.** A nested key present in the target but absent from the
    deployment's config is not dropped — the guard against the wholesale-replace trap in the
    implementation note.
14. **Non-nil baseline unchanged.** Existing three-way behaviour is untouched, so the
    cross-config-name case (Q4) keeps carrying divergences.

Plus `mise run go:lint`.

## Resolved decisions

All four questions were answered by the reviewer on turn 1, and turn 2 approved the spec with one
further instruction — *"let's have a browsable collapsible list"* — which is folded in as the tree
design above. No open questions remain.

| # | Question | Decision |
|---|---|---|
| Q1 | Picker shape | **Three-level: project → config → version.** Cross-project adoption is consequently in scope. |
| Q5 | Presentation (turn 2) | **One browsable collapsible tree**, not three sequential `huh.Select` prompts. Same three levels; built on the existing `fieldTree`. Costs type-to-filter — see *Follow-ups*. |
| Q2 | Project level when it cannot be inferred | **Auto-expand the sole project when the org has exactly one**, otherwise leave projects collapsed — including for linked deployments, whose own project is auto-expanded regardless. |
| Q3 | Unlinked deployment adopting a config | **Preserve** the deployment's current configuration as overrides (decision A), via a recursing two-way deep merge. |
| Q4 | Switching across config names | **Keep** the three-way divergence merge against the previously-linked baseline (decision B); confirm copy says so. |

Context worth carrying forward: `schemasv1.DeploymentSchema` carries **no** benchmark-project
field (only `DeployConfigID` / `DeployConfigVersion`, `deployment.go:52`), which is why the
project must come from the picker or from the linked config rather than from the deployment.

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
