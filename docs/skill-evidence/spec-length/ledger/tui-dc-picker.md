# Key-point ledger — `tui-dc-picker`

Derived from `../fixtures/tui-dc-picker.spec.md` and **nothing else**. No prompt text — no arm, no
phase prompt, no plan — was read to decide a row. See `../FREEZE.md` for why that is the whole
point, and for this file's hash.

A row is **load-bearing**: an implementer holding a candidate spec that omitted it would build
something materially different, or would have to stop and ask. Rationale, motivation, history, and
examples illustrating a point already stated are not rows. `kind` is one of `decision`,
`interface`, `constraint`, `scope`. Ids are stable forever — a later task may not renumber them.

**Closed list: 55 rows.**

| id | kind | item |
|---|---|---|
| tui-dc-picker-01 | constraint | Every listing call must pass an explicit count parameter (`srcConfigListCount` / `deployConfigVersionsCount`) because an omitted `count` arrives as `LIMIT 0` rather than unlimited. |
| tui-dc-picker-02 | decision | The picker has three levels: benchmark project → deploy config → version. |
| tui-dc-picker-03 | decision | The picker is one browsable collapsible tree built on the existing `fieldTree` widget (`browse_fieldtree.go`), not a chain of modal `huh.Select` steps. |
| tui-dc-picker-04 | interface | `↑`/`↓` move the cursor over visible nodes via `fieldTree.up`/`down`. |
| tui-dc-picker-05 | interface | `→`/`l` expands and `←`/`h` collapses via `fieldTree.setCollapsed`, with `space` toggling via `fieldTree.toggle`. |
| tui-dc-picker-06 | interface | `enter` on a version leaf chooses that version and arms the confirm. |
| tui-dc-picker-07 | constraint | `enter` on a project or config branch only toggles it — a branch is never a selection. |
| tui-dc-picker-08 | interface | `esc` cancels the whole picker and sets `statusDCCanceled`. |
| tui-dc-picker-09 | decision | The project level is loaded when the picker opens, via one `ListBenchmarkProjects` request. |
| tui-dc-picker-10 | decision | A project's configs are fetched only when that project is first expanded, via `ListDeployConfigs(project)` + `latestPerConfig`. |
| tui-dc-picker-11 | decision | A config's versions are fetched only when that config is first expanded, via `fetchDeployConfigVersions(project, name)`. |
| tui-dc-picker-12 | constraint | A never-expanded branch holds a single placeholder child rendering `loading…`, replaced on arrival and shown as `(none)` when the listing is empty. |
| tui-dc-picker-13 | constraint | Each branch caches its children after the first load so re-collapsing and re-expanding does not refetch. |
| tui-dc-picker-14 | decision | On open with a linked deployment, auto-expand its project and its config and place the cursor on the current version. |
| tui-dc-picker-15 | decision | On open with an unlinked deployment, auto-expand the sole project when the org has exactly one, otherwise leave all projects collapsed with the cursor on the first. |
| tui-dc-picker-16 | scope | Type-to-filter in the picker is out of scope — `fieldTree` has no equivalent of `huh.Select`'s `.Filtering(true)`, so the tree is cursor-navigated only. |
| tui-dc-picker-17 | scope | Cross-project adoption is in scope: every project in the org is a root of the tree, so a deployment can be switched to a config in any project. |
| tui-dc-picker-18 | interface | `completeDCSelection` populates `confirmState.dcProject`/`dcName` from the chosen leaf's config rather than from the linked config. |
| tui-dc-picker-19 | interface | `appModel` gains `dcTree *fieldTree`, non-nil exactly while the picker is open. |
| tui-dc-picker-20 | interface | `appModel` gains `dcLinked *schemasv1.DeployConfigSchema`, the linked config that drives auto-expand, cursor placement, and the merge baseline handed to the confirm. |
| tui-dc-picker-21 | interface | `appModel` gains `dcLoading map[string]bool` tracking in-flight branch loads keyed `""` for projects, `"<project>"`, and `"<project>\x00<config>"`. |
| tui-dc-picker-22 | decision | The existing `dcForm`, `dcVersions`, `dcCurrent`, `dcChoice`, and `dcVersLoading` state is removed and replaced by the tree state. |
| tui-dc-picker-23 | interface | `fieldNode` gains an optional `pick any` field, set to the `*schemasv1.DeployConfigSchema` on picker version leaves. |
| tui-dc-picker-24 | constraint | `pick` must remain nil on every node the existing config, revision, and diff tree builders produce. |
| tui-dc-picker-25 | interface | `fieldTree` gains a `current() *fieldNode` accessor returning the node under the cursor so the picker never touches the unexported `flat`/`cursor` internals. |
| tui-dc-picker-26 | interface | New message `dcProjectsMsg struct { name string; items []*schemasv1.BenchmarkProjectSchema; err error }`. |
| tui-dc-picker-27 | interface | New message `dcConfigsMsg struct { name, project string; items []*schemasv1.DeployConfigSchema; err error }`. |
| tui-dc-picker-28 | interface | New message `dcVersionsMsg struct { name, project, config string; items []*schemasv1.DeployConfigSchema; err error }`. |
| tui-dc-picker-29 | constraint | `name` on all three messages is the deployment name and results are dropped when `msg.name != m.sel.Name`. |
| tui-dc-picker-30 | constraint | A result must fill the branch identified by its `project`/`config` fields rather than the branch under the cursor. |
| tui-dc-picker-31 | interface | `currentOverlay()` keeps a single arm `case m.dcTree != nil:` returning `overlayHandler{m.updateDCPicker, m.dcPickerView}`. |
| tui-dc-picker-32 | constraint | `autoRefreshPaused()` gates on `m.dcTree != nil` instead of `m.dcVersLoading`, so auto-refresh cannot swap `m.sel` out while the picker is open, including while idle between branch loads. |
| tui-dc-picker-33 | decision | The `DeployConfigID == nil` rejection at `browse_model.go:1910-1917` is dropped so `V` is valid on any detail-screen selection. |
| tui-dc-picker-34 | interface | `helpAdapter.dcLinked` (`browse_model.go:2916`) loses its `DeployConfigID != nil` condition so `V` is advertised on unlinked deployments. |
| tui-dc-picker-35 | interface | A config node is labelled `"<name>  (latest v<N>)"`. |
| tui-dc-picker-36 | interface | `"  (shared)"` is appended to a config node when the schema's `Shared` field is true. |
| tui-dc-picker-37 | interface | `"  (current)"` marks the linked config's node and the deployment's current version leaf. |
| tui-dc-picker-38 | interface | A version leaf is labelled `"v<N>  <created>  <description>"`, reusing the label `newDeployConfigVersionPicker` builds with `trunc`/`sanitizeCell`. |
| tui-dc-picker-39 | interface | `"  (latest)"` marks the newest version leaf of a config. |
| tui-dc-picker-40 | interface | The panel label in `browse_panels.go:226-228` changes from `"V: switch version"` to `"V: switch deploy config"`. |
| tui-dc-picker-41 | decision | When the linked baseline is absent, `preserveDeploymentDivergences` stops returning the target verbatim and instead deep-merges with the target config as base and the deployment's current configuration as the overlay, the overlay winning on conflicting leaves while target-only keys still land. |
| tui-dc-picker-42 | constraint | The two-way merge must recurse and be implemented as a distinct map-level helper (the map twin of `mergeJSONPtr`), not as `threeWayMerge` called with an empty-map baseline. |
| tui-dc-picker-43 | decision | Switching across config names keeps the three-way divergence merge against the previously-linked baseline. |
| tui-dc-picker-44 | interface | The `actionSwitchDeployConfig` confirm states that switching redeploys the deployment. |
| tui-dc-picker-45 | interface | The confirm states that the deployment's overrides are carried across. |
| tui-dc-picker-46 | interface | When the chosen config is shared, the confirm states that the next `mcloud deploy-config apply` creates a new version from the shared spec and moves the config's latest, while the deployment stays pinned to the version selected here. |
| tui-dc-picker-47 | scope | In scope: `V` reaches every deploy config in the project, shared configs included. |
| tui-dc-picker-48 | scope | The `preserveDeploymentDivergences` change also fixes the same gap on the existing `mcloud deployment deploy-config set` path that shares it. |
| tui-dc-picker-49 | scope | No API-server, transport, or schema change is made. |
| tui-dc-picker-50 | scope | The create/edit source picker (`browse_source_config.go`) and the sync picker (`browse_form_sync.go`) keep their `huh.Select` steps and are not converted to the tree. |
| tui-dc-picker-51 | scope | No warning about shared-config edits being reverted is added, because the TUI and CLI have no deploy-config write path at all. |
| tui-dc-picker-52 | scope | `browse_source_config.go` and the new `V` flow are not refactored into one generic picker; only the shared data helpers `latestPerConfig`, `findDeployConfigVersion`, and `fetchDeployConfigVersions` are reused as-is. |
| tui-dc-picker-53 | constraint | Picker tests are table-driven model tests in `browse_deploy_config_test.go` driving `appModel.Update` with synthetic messages and a nil client. |
| tui-dc-picker-54 | constraint | Merge-semantics tests live in `deploy_config_test.go` as table-driven pure-function tests on `preserveDeploymentDivergences` with no model. |
| tui-dc-picker-55 | constraint | `mise run go:lint` must pass. |

## Derivation notes

Recorded so a later reader can see what was weighed, not only what survived. These are the deriving
subagent's own exclusions, kept verbatim in substance:

- The numbered test cases (1–11, 12–14) were excluded individually: each restates a rule already
  carried by another row. Only the test *shape* rows (53–55) survive.
- The whole Problem section (name-pinned fetch, entry-point gate, server verification) and the
  "Why not a flat list" / "Cost of this shape" arguments were excluded as diagnosis and rationale.
- `dcVersionsMsg` losing its `current uint` field was excluded: it follows from `dcLinked` being
  the source of the current version (row 20).
- The project-node label (the bare project name) was excluded as too trivial to change an
  implementation.
- The Follow-ups section (filtering in `fieldTree`, converting the create/edit picker) was excluded
  as explicitly "not this change", and is already bounded by rows 16 and 50.
- The "Files touched" and "Git pointers" tables were excluded as restatements and environment
  detail, as was the closing note that `DeploymentSchema` carries no benchmark-project field
  (rationale for row 18).
