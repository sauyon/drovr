//! Backend-agnostic agent map + review angles.
//!
//! Loaded from `${XDG_CONFIG_HOME:-~/.config}/drovr/config.toml`, falling back to
//! baked-in defaults when the file is absent. Resolves a reviewer's launch command
//! and its read-only flag.
//!
//! User-defined agents override built-ins by key. Missing built-in agents and
//! missing optional fields on a built-in agent are filled in for compatibility.

use std::collections::BTreeMap;
use std::io;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use crate::herdr::SessionId;
use crate::shell::shell_single_quote;

/// The document shape a backend expects its MCP servers written in.
///
/// Orthogonal to *how* the file reaches the backend ([`McpDelivery`]): the
/// mechanism decides where the file goes, the schema decides what is in it.
///
/// Deliberately has NO default. `load_config` replaces an overridden `mcp` table
/// wholesale, so a default would mean an `[agents.opencode.mcp]` stanza that only
/// retunes the path quietly starts writing cursor's schema into `opencode.json` —
/// which opencode parses without complaint and reads no servers from, leaving a
/// review panel with no findings channel and no error to explain it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum McpSchema {
    /// `{"mcpServers": {"<name>": {"command": …, "args": […]}}}` — claude, cursor.
    McpServers,
    /// `{"mcp": {"<name>": {"type": "local", "command": […], "enabled": true}}}` —
    /// opencode, whose file is its whole project config rather than an MCP file.
    Opencode,
}

/// How a backend is handed an MCP server.
///
/// The two supported mechanisms are genuinely different, so they are separate
/// variants rather than a pile of optional flags that could be combined into
/// nonsense: claude takes the server config *file* on its command line, while
/// cursor and opencode have no such flag at all and only read a fixed path
/// inside the project directory. A backend either names the file or it does not
/// — never both.
///
/// `deny_unknown_fields` for the same reason [`AgentSpec`] carries it: a typo in
/// a key that decides what drovr writes into someone's project is a config error,
/// not a comment. It works with [`McpSchema`]'s absent default rather than around
/// it — `deny_unknown_fields` catches `schemas = "opencode"`, and the missing
/// default catches the stanza that never mentions a schema at all.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(tag = "mechanism", rename_all = "kebab-case", deny_unknown_fields)]
pub enum McpDelivery {
    /// The config file is named on the command line: `<flag> <path>`, plus any
    /// `extra_flags` (claude: `--mcp-config <path> --strict-mcp-config`, the
    /// second flag confining the agent to exactly the servers drovr passed).
    ConfigFlag {
        flag: String,
        #[serde(default)]
        extra_flags: Vec<String>,
        schema: McpSchema,
    },
    /// The backend reads servers only from a fixed project-relative path
    /// (cursor: `.cursor/mcp.json`, opencode: `opencode.json`); `extra_flags`
    /// carries whatever makes it trust that file without a prompt
    /// (`--approve-mcps`).
    ProjectFile {
        path: String,
        #[serde(default)]
        extra_flags: Vec<String>,
        schema: McpSchema,
    },
}

impl McpDelivery {
    /// Where drovr must write the server config for this mechanism. A flag
    /// backend reads it from drovr's own run directory, out of the project
    /// entirely; a project-file backend only ever looks inside the project.
    ///
    /// `stem` names the file for a flag backend (the reviewers of one task share
    /// one server, so it is the task name).
    pub fn config_path(&self, run_dir: &Path, project_dir: &Path, stem: &str) -> PathBuf {
        match self {
            McpDelivery::ConfigFlag { .. } => run_dir.join(format!("{stem}-review-mcp.json")),
            McpDelivery::ProjectFile { path, .. } => project_dir.join(path),
        }
    }

    /// The project-relative path this mechanism writes into the user's checkout,
    /// if any. `Some` means the file is visible to git and needs excluding.
    pub fn project_relative_path(&self) -> Option<&str> {
        match self {
            McpDelivery::ConfigFlag { .. } => None,
            McpDelivery::ProjectFile { path, .. } => Some(path),
        }
    }

    fn extra_flags(&self) -> &[String] {
        match self {
            McpDelivery::ConfigFlag { extra_flags, .. }
            | McpDelivery::ProjectFile { extra_flags, .. } => extra_flags,
        }
    }

    /// The document shape to write at [`McpDelivery::config_path`].
    pub fn schema(&self) -> McpSchema {
        match self {
            McpDelivery::ConfigFlag { schema, .. } | McpDelivery::ProjectFile { schema, .. } => {
                *schema
            }
        }
    }
}

/// How a backend is told which directory the run's project is.
///
/// Two genuinely different shapes, so — like [`McpDelivery`] — an enum rather
/// than optional fields that could be set into nonsense: a backend either names
/// the directory behind a flag or takes it as a bare positional, never both.
///
/// The distinction is not cosmetic. opencode's argument parser accepts unknown
/// options silently, so inventing a flag for it would compose a command that
/// *looks* pinned to the project and runs unpinned.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(tag = "mechanism", rename_all = "kebab-case", deny_unknown_fields)]
pub enum WorkspaceArg {
    /// `<flag> <dir>` (claude: `--add-dir`, cursor: `--workspace`, codex: `-C`).
    Flag { flag: String },
    /// A bare `<dir>` with no flag word (opencode: `opencode <project>`).
    Positional,
}

/// `deny_unknown_fields` is load-bearing, not tidiness: every field here is a
/// switch that changes what drovr spawns, and serde's default is to ignore keys
/// it does not recognise. A config carrying the pre-[`WorkspaceArg`] spelling
/// (`workspace_flag = "--add-dir"`) would otherwise load clean and leave the
/// agent silently *unpinned* from the project. Fail the load instead.
/// How an agent is told to resume a prior session. **One field, one shape** —
/// the two TOML keys fold into this on the way in.
///
/// Resume is not one shape across backends: claude `-r, --resume [value]` and
/// cursor `agent --resume [chatId]` are FLAGS whose value is optional, while
/// codex takes a `codex resume <id>` SUBCOMMAND, which binds to the command and
/// must precede every flag. They are mutually exclusive, and this is where that
/// is *said by the type* rather than promised by a doc comment — an `AgentSpec`
/// claiming both shapes is not constructible, in memory or from a config file.
///
/// The optional value is why the id is never separable from the surface: a bare
/// `--resume` opens an interactive session PICKER and parks the pane forever.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResumeSpec {
    /// `<command> … <flag> '<id>'` — where the other flags go.
    Flag(ResumeToken),
    /// `<command> <subcommand> '<id>' …` — immediately after the command.
    Subcommand(ResumeToken),
}

/// The flag or subcommand word itself — **never empty**, by construction.
///
/// A newtype over a private `String` rather than a bare `String` in the variant,
/// because an empty token is not a cosmetic defect: `resume_flag = ""` composes
/// `<command>  '<id>'`, handing the session id to the agent as a positional
/// argument, and an empty *flag* is exactly how a bare `--resume` reaches the
/// shell — which opens claude's interactive session picker and parks the pane
/// forever, with no human anywhere near it.
///
/// The check used to live in `TryFrom<AgentSpecWire>`, i.e. on the config-file
/// path only. Everything else — the built-in map, tests, any future caller —
/// was on the honour system, and `ResumeSpec::Flag(String::new())` was one
/// expression away. Enum variants are as public as their enum, so the guard
/// cannot live on the variant; it lives here, on the only value the variants can
/// hold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResumeToken(String);

impl ResumeSpec {
    /// A flag-shaped resume surface (claude, cursor). `Err` names the problem
    /// for a config file to print verbatim.
    pub fn flag(token: impl Into<String>) -> Result<ResumeSpec, String> {
        ResumeToken::new(token, "resume_flag").map(ResumeSpec::Flag)
    }

    /// A subcommand-shaped resume surface (codex). Same validation.
    pub fn subcommand(token: impl Into<String>) -> Result<ResumeSpec, String> {
        ResumeToken::new(token, "resume_subcommand").map(ResumeSpec::Subcommand)
    }

    /// The flag or subcommand text, for composing. Non-empty by construction.
    fn token(&self) -> &str {
        match self {
            ResumeSpec::Flag(t) | ResumeSpec::Subcommand(t) => &t.0,
        }
    }
}

impl ResumeToken {
    /// The ONLY constructor. `key` names the config field in the error, so the
    /// message reads the same whether the value came from a file or from code.
    fn new(token: impl Into<String>, key: &str) -> Result<ResumeToken, String> {
        let token = token.into();
        if token.trim().is_empty() {
            return Err(format!("{key} must not be empty"));
        }
        Ok(ResumeToken(token))
    }
}

/// The wire shape of an agent entry: two optional resume keys, as a config file
/// spells them. [`AgentSpec`] is built from this via `TryFrom`, which is where
/// "both at once" and "empty" are rejected — so the validated type downstream
/// cannot express either.
///
/// `deny_unknown_fields` because listing a field decides what a config file CAN
/// set, but not what happens to everything else — and serde's default is to
/// ignore an unrecognised key in silence. Every key here changes what drovr
/// spawns or writes into someone's project, so a config written against the old
/// `workspace_flag = "…"` spelling must fail the load rather than load clean and
/// leave the agent silently unpinned.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct AgentSpecWire {
    command: String,
    #[serde(default)]
    readonly_flag: Option<String>,
    /// See [`AgentSpec::readonly_env_unset`].
    #[serde(default)]
    readonly_env_unset: Vec<String>,
    /// See [`AgentSpec::readonly_displace`].
    #[serde(default)]
    readonly_displace: Vec<String>,
    /// See [`AgentSpec::workspace`]. A *mechanism*, not a flag string: opencode
    /// names its project positionally, and its parser ignores unknown options
    /// silently, so a made-up flag would compose a command that looks pinned to
    /// the project and runs unpinned.
    #[serde(default)]
    workspace: Option<WorkspaceArg>,
    #[serde(default)]
    system_prompt_flag: Option<String>,
    #[serde(default)]
    model_flag: Option<String>,
    #[serde(default)]
    review_model: Option<String>,
    /// Flag that resumes a prior session (claude, cursor).
    #[serde(default)]
    resume_flag: Option<String>,
    /// Subcommand that resumes a prior session. No built-in sets this:
    /// `codex resume <id>` is the known shape, but codex was not installed on
    /// the machine this was written on, so its argument ordering could not be
    /// verified — and an unverified guess composes a wrong command line where
    /// `None` merely reseeds. A codex user opts in explicitly:
    ///
    /// ```toml
    /// [agents.codex]
    /// resume_subcommand = "resume"
    /// ```
    #[serde(default)]
    resume_subcommand: Option<String>,
    /// How this backend is handed an MCP server. Carried through the wire type
    /// unchanged — it has no validation of its own, but a field that is not
    /// listed here is a field a config file cannot set at all.
    #[serde(default)]
    mcp: Option<McpDelivery>,
}

impl TryFrom<AgentSpecWire> for AgentSpec {
    type Error = String;
    fn try_from(w: AgentSpecWire) -> Result<AgentSpec, String> {
        // Emptiness is rejected by `ResumeToken`, not here — see that type for
        // why the guard cannot sit on this path alone. This match decides only
        // WHICH shape was spelled, and refuses both at once.
        let resume = match (w.resume_flag, w.resume_subcommand) {
            (Some(flag), None) => Some(ResumeSpec::flag(flag)?),
            (None, Some(sub)) => Some(ResumeSpec::subcommand(sub)?),
            (None, None) => None,
            (Some(_), Some(_)) => {
                return Err(
                    "resume_flag and resume_subcommand are mutually exclusive; set exactly one"
                        .into(),
                );
            }
        };
        Ok(AgentSpec {
            command: w.command,
            readonly_flag: w.readonly_flag,
            readonly_env_unset: w.readonly_env_unset,
            readonly_displace: w.readonly_displace,
            workspace: w.workspace,
            system_prompt_flag: w.system_prompt_flag,
            model_flag: w.model_flag,
            review_model: w.review_model,
            resume,
            mcp: w.mcp,
        })
    }
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(try_from = "AgentSpecWire")]
pub struct AgentSpec {
    pub command: String,
    /// Read-only flag; absent → this agent cannot serve as a reviewer.
    pub readonly_flag: Option<String>,
    /// Environment variables cleared from a **read-only** launch because they
    /// redirect the backend's config discovery, and drovr's control of that file
    /// is what confines a reviewer to the one tool it is meant to have.
    ///
    /// Read-only only, and deliberately so. drovr replaces the project config for a
    /// reviewer and for nobody else, so there is no guarantee to protect on a writer
    /// phase — and clearing the var there would break a user who points it at their
    /// real provider config for no gain.
    pub readonly_env_unset: Vec<String>,
    /// Project-relative paths moved aside for a **read-only** launch because the
    /// backend reads agent, plugin or tool definitions from them — which means the
    /// repository under review can define the read-only agent itself.
    ///
    /// opencode's `.opencode/` is the case this exists for: `--agent plan` names a
    /// *definition*, not a CLI flag, and a committed `.opencode/agent/plan.md`
    /// replaces the one drovr configured. claude and cursor need nothing here because
    /// their read-only modes are flags the checkout cannot reach.
    ///
    /// The same read-only-only rule as [`AgentSpec::readonly_env_unset`], and the same
    /// union merge: these are invariants of the backend, not defaults to choose
    /// between.
    pub readonly_displace: Vec<String>,
    /// How the agent is pinned to the run's project directory.
    pub workspace: Option<WorkspaceArg>,
    /// Flag used to append the workspace-root guard prompt.
    pub system_prompt_flag: Option<String>,
    /// Flag used to select a model for read-only reviews.
    pub model_flag: Option<String>,
    /// Model selected for read-only reviews. Absent means backend default.
    pub review_model: Option<String>,
    /// How this agent resumes a session, if it can — see [`ResumeSpec`].
    /// Absent → no resume surface, and a rehydrate degrades to a reseed.
    pub resume: Option<ResumeSpec>,
    /// How this backend is given an MCP server. Absent → it cannot be handed
    /// one, so it cannot serve on the review panel (whose findings channel is a
    /// tool call).
    pub mcp: Option<McpDelivery>,
}

/// Controls the SessionStart reflex the `session-start` hook injects (see
/// `drovr reflex`). All fields are optional; an absent `[reflex]` table yields
/// the built-in reflex unchanged.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct ReflexConfig {
    /// Master switch. `false` suppresses the reflex entirely for human sessions
    /// (the `DROVR_PHASE` phase-suppression is separate and always applies).
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Overrides the framing text placed before the skill body inside the
    /// `<EXTREMELY_IMPORTANT>` wrapper. Absent → the built-in framing.
    #[serde(default)]
    pub preamble: Option<String>,
    /// Per-section overrides keyed by the section name tagged in the skill
    /// markdown (`<!-- reflex:section:NAME -->`). A section absent from this map
    /// defaults to enabled; `NAME = false` omits that section from the reflex.
    #[serde(default)]
    pub sections: BTreeMap<String, bool>,
}

impl Default for ReflexConfig {
    fn default() -> Self {
        ReflexConfig {
            enabled: true,
            preamble: None,
            sections: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct Config {
    #[serde(default = "default_agent")]
    pub default_agent: String,
    /// Backend used for automated review panels. When omitted, prefer Cursor's
    /// `agent` backend if its command is available, then fall back to the run backend.
    #[serde(default)]
    pub review_agent: Option<String>,
    #[serde(default = "default_angles")]
    pub angles: Vec<String>,
    /// Default host/address `drovr serve` binds to when `--host` is omitted.
    #[serde(default = "default_serve_host")]
    pub serve_host: String,
    /// When true, `drovr new` isolates each run in a git worktree
    /// (`.drovr/wt/<run>` on branch `drovr/<run>`) unless `--no-worktree` is
    /// passed. `--worktree` overrides this to on per-run. **On by default** —
    /// isolation keeps a run's edits off the checkout you launched it from, so
    /// `default_true` (not a bare `#[serde(default)]`, which would yield `false`
    /// whenever a config file omits the key).
    #[serde(default = "default_true")]
    pub worktree: bool,
    /// When true, drovr closes a phase's herdr pane once the run has provably
    /// moved past it — `phase_start` reaps every other finished phase after its
    /// own launch succeeds, and `code_review_run` reaps its panel once the
    /// findings are merged. See `phase::phase_reap`.
    ///
    /// **On by default**, so `default_true` and not a bare `#[serde(default)]`,
    /// which yields `false` whenever a config file omits the key — the same trap
    /// `worktree` above documents. Set `reap_finished_panes = false` to keep
    /// every pane until `drovr cleanup`, which is what drovr did before reaping
    /// existed; nothing else changes, and `drovr phase reap` still works when
    /// asked for explicitly, because that is a command rather than a policy.
    #[serde(default = "default_true")]
    pub reap_finished_panes: bool,
    /// SessionStart reflex configuration (see [`ReflexConfig`]).
    #[serde(default)]
    pub reflex: ReflexConfig,
    #[serde(default = "default_agents")]
    pub agents: BTreeMap<String, AgentSpec>,
}

// Standalone default fns are REQUIRED (not `#[serde(default)]` bare): serde's bare default
// calls `BTreeMap::default()` (EMPTY) when the TOML omits `[agents]`, which would make the
// built-in claude entry vanish on any real config file and break `reviewer_launch("claude")`.
// The same trap applies to non-empty String defaults like `serve_host`: bare `#[serde(default)]`
// yields `String::default()` (`""`), not `"127.0.0.1"`, so it needs a named fn too.
// Each default fn seeds the built-in value so an absent field falls back correctly.
fn default_agent() -> String {
    "claude".into()
}

fn default_serve_host() -> String {
    "127.0.0.1".into()
}

// Named `true` default for `ReflexConfig::enabled`: a bare `#[serde(default)]`
// on a bool yields `false`, which would silently disable the reflex whenever a
// `[reflex]` table is present but omits `enabled`.
fn default_true() -> bool {
    true
}

fn default_angles() -> Vec<String> {
    vec![
        "correctness".into(),
        "security".into(),
        "error-handling".into(),
        "type-design".into(),
    ]
}

/// The resume surface claude and cursor share (`-r, --resume [value]` on both,
/// verified against the real CLIs as a flag whose value is OPTIONAL).
///
/// One constructor rather than two literals: `ResumeSpec::flag` is fallible, so
/// two call sites would be two `expect`s, and the built-in map is exactly the
/// place where "the token is obviously non-empty" would stop being checked.
fn builtin_resume_flag() -> ResumeSpec {
    ResumeSpec::flag("--resume").expect("the built-in resume flag is non-empty")
}

fn default_agents() -> BTreeMap<String, AgentSpec> {
    let mut m = BTreeMap::new();
    m.insert(
        "claude".to_string(),
        AgentSpec {
            command: "claude".into(),
            readonly_flag: Some("--permission-mode plan".into()),
            readonly_env_unset: Vec::new(),
            readonly_displace: Vec::new(),
            workspace: Some(WorkspaceArg::Flag {
                flag: "--add-dir".into(),
            }),
            system_prompt_flag: Some("--append-system-prompt".into()),
            model_flag: Some("--model".into()),
            review_model: None,
            // Verified against the real CLI: `claude -r, --resume [value]` —
            // a flag whose value is OPTIONAL.
            resume: Some(builtin_resume_flag()),
            mcp: Some(McpDelivery::ConfigFlag {
                flag: "--mcp-config".into(),
                // `--strict-mcp-config`: exactly the servers drovr passed, none of
                // the user's. `--allowedTools`: plan mode gates tool use, so the one
                // tool the panel depends on is pre-allowed — and only that one, whose
                // single effect is drovr writing the findings file.
                // `--allowedTools` is VARIADIC: given as two argv words it swallows
                // whatever follows, so it is passed in the `=` form, which closes the
                // option and cannot consume the next flag.
                extra_flags: vec![
                    "--strict-mcp-config".into(),
                    format!(
                        "--allowedTools={}",
                        crate::mcp_findings::qualified_tool_name()
                    ),
                ],
                schema: McpSchema::McpServers,
            }),
        },
    );
    m.insert(
        "cursor".to_string(),
        AgentSpec {
            command: "agent".into(),
            readonly_flag: Some("--mode plan".into()),
            readonly_env_unset: Vec::new(),
            readonly_displace: Vec::new(),
            workspace: Some(WorkspaceArg::Flag {
                flag: "--workspace".into(),
            }),
            system_prompt_flag: None,
            model_flag: Some("--model".into()),
            review_model: Some("composer-2.5".into()),
            // Verified against the real CLI: `agent --resume [chatId]`.
            resume: Some(builtin_resume_flag()),
            // No per-launch scoping exists: servers come from the project's
            // `.cursor/mcp.json`, so drovr writes that file instead.
            //
            // `--approve-mcps` approves EVERY server in that file — there is no
            // per-server form. That is only safe because
            // `code_review::write_mcp_config` makes drovr's findings server the sole
            // entry, backing up anything it displaces. Preserving other servers here
            // would auto-approve them for a read-only reviewer, and `.cursor/mcp.json`
            // is a path the repository under review can simply commit. If that write
            // is ever made to merge again, this flag has to go with it.
            mcp: Some(McpDelivery::ProjectFile {
                path: ".cursor/mcp.json".into(),
                extra_flags: vec!["--approve-mcps".into()],
                schema: McpSchema::McpServers,
            }),
        },
    );
    m.insert(
        "codex".to_string(),
        AgentSpec {
            command: "codex".into(),
            readonly_flag: Some("--sandbox read-only".into()),
            readonly_env_unset: Vec::new(),
            readonly_displace: Vec::new(),
            workspace: Some(WorkspaceArg::Flag { flag: "-C".into() }),
            system_prompt_flag: None,
            model_flag: Some("-m".into()),
            review_model: None,
            resume: None,
            mcp: None,
        },
    );
    m.insert(
        "opencode".to_string(),
        AgentSpec {
            command: "opencode".into(),
            // None, on the same rule the codex entry above states: an unverified
            // guess composes a wrong command line, where `None` merely reseeds.
            // opencode does advertise `-s, --session <id>` (a flag whose value is
            // required, so it cannot fall into the bare-`--resume` session-picker
            // trap), but that it actually restores the conversation could not be
            // verified here — the provider on this machine would not answer. A
            // user opts in explicitly:
            //
            //     [agents.opencode]
            //     resume_flag = "--session"
            resume: None,
            // opencode's read-only stance is its built-in `plan` agent. That agent
            // only sets edits and bash to *ask*, which stalls an unattended reviewer
            // pane rather than refusing — so drovr writes the permission overrides
            // that turn it into a real read-only stance alongside the MCP server
            // (see `code_review::opencode_document`).
            readonly_flag: Some("--agent plan".into()),
            // Replacing the project `opencode.json` is what strips a repository's own
            // servers — but opencode takes config from the environment too, and every
            // one of these MERGES over the project file rather than deferring to it,
            // so an inherited value puts those servers straight back. Clearing them is
            // part of the same guard, not a backstop for it.
            //
            // All four probed against opencode 1.18.3 with `opencode debug config`,
            // against a project file holding drovr's server:
            //   * OPENCODE_CONFIG         — names another config file
            //   * OPENCODE_CONFIG_CONTENT — inline JSON
            //   * OPENCODE_CONFIG_DIR     — another directory to read one from
            // each resolve to their own server ALONGSIDE `drovr-findings`, and
            //   * OPENCODE_PERMISSION     — sets the permission block wholesale,
            // which is the other half of what drovr writes (see
            // `code_review::opencode_document`). Shutting one door is not a guard.
            readonly_env_unset: vec![
                "OPENCODE_CONFIG".into(),
                "OPENCODE_CONFIG_CONTENT".into(),
                "OPENCODE_CONFIG_DIR".into(),
                "OPENCODE_PERMISSION".into(),
            ],
            // `--agent plan` names a definition the checkout can replace: a committed
            // `.opencode/agent/plan.md` takes drovr's `edit: deny` out of the resolved
            // rules entirely (probed, 1.18.3), and `.opencode/plugin/*.js` loads as
            // arbitrary JS in the reviewer's process. The WHOLE directory, because
            // drovr cannot know which parts of it confer capability in an opencode
            // release it has never seen — and a subdirectory whitelist is a second
            // description of opencode's layout that would drift out of date here.
            readonly_displace: vec![".opencode".into()],
            workspace: Some(WorkspaceArg::Positional),
            system_prompt_flag: None,
            model_flag: Some("--model".into()),
            review_model: None,
            // No per-launch scoping and no config flag: opencode merges a project
            // `opencode.json` over its global config, so that file is the only place
            // drovr can put a server. `OPENCODE_CONFIG` names an external file and is
            // the tidier mechanism, but it *merges* with the repository's own
            // `opencode.json` rather than displacing it — a repository under review
            // could then hand its own servers to a read-only reviewer. Replacing the
            // project file (with the backup that `write_mcp_config` performs) is what
            // strips them, exactly as for cursor.
            mcp: Some(McpDelivery::ProjectFile {
                path: "opencode.json".into(),
                // Nothing to approve: opencode takes servers from the config it has
                // merged, without a trust prompt.
                extra_flags: Vec::new(),
                schema: McpSchema::Opencode,
            }),
        },
    );
    m
}

/// Detect the agent backend invoking drovr.
///
/// `DROVR_AGENT` is an explicit escape hatch for agents without a stable
/// environment marker. Otherwise use markers exported by the major CLIs and
/// fall back to the configured default when drovr is run from an ordinary shell.
pub fn invoking_agent(config: &Config) -> String {
    if let Ok(agent) = crate::env::var("DROVR_AGENT")
        && !agent.trim().is_empty()
    {
        return agent;
    }
    if crate::env::var_os("CURSOR_AGENT").is_some() {
        return "cursor".into();
    }
    if crate::env::var_os("CLAUDECODE").is_some() {
        return "claude".into();
    }
    if crate::env::var_os("CODEX_THREAD_ID").is_some() {
        return "codex".into();
    }
    if crate::env::var_os("OPENCODE").is_some() {
        return "opencode".into();
    }
    config.default_agent.clone()
}

impl Default for Config {
    fn default() -> Self {
        Config {
            default_agent: default_agent(),
            review_agent: None,
            angles: default_angles(),
            serve_host: default_serve_host(),
            worktree: default_true(),
            reap_finished_panes: default_true(),
            reflex: ReflexConfig::default(),
            agents: default_agents(),
        }
    }
}

/// `${XDG_CONFIG_HOME:-$HOME/.config}/drovr/config.toml`
pub fn config_path() -> PathBuf {
    let base = crate::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            // `HOME` unset (CI/containers) must not panic: fall back to a relative
            // `.config`, which `load_config` will simply see as NotFound → defaults.
            let home = crate::env::var("HOME").unwrap_or_default();
            PathBuf::from(home).join(".config")
        });
    base.join("drovr").join("config.toml")
}

/// Load the config. Absent file → `Ok(Config::default())`; present-but-malformed → `Err`.
pub fn load_config() -> io::Result<Config> {
    let path = config_path();
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Config::default()),
        Err(e) => return Err(e),
    };
    let mut config: Config = toml::from_str(&text).map_err(io::Error::other)?;
    // An empty `serve_host` is a config mistake, not a request for the default
    // (serde's default only fires for an *absent* key). Reject it here so the
    // failure is a clear config error rather than an opaque bind error later.
    if config.serve_host.trim().is_empty() {
        return Err(io::Error::other("serve_host must not be empty"));
    }
    // Reject relative-path commands before any of them can be spawned. A
    // hostile repo under review (or tampered config) must not be able to point
    // drovr at a CWD-relative binary.
    for (name, spec) in &config.agents {
        validate_command(&spec.command)
            .map_err(|e| io::Error::other(format!("agent '{name}': {e}")))?;
        for path in &spec.readonly_displace {
            validate_project_relative(path, "`readonly_displace` entry")
                .map_err(|e| io::Error::other(format!("agent '{name}': {e}")))?;
        }
        if let Some(delivery) = &spec.mcp {
            validate_delivery(delivery)
                .map_err(|e| io::Error::other(format!("agent '{name}': {e}")))?;
        }
    }
    for (name, builtin) in default_agents() {
        if let Some(spec) = config.agents.get_mut(&name) {
            // One field, so this is an ordinary `.or()` like every line below
            // it — the special-case block this used to need disappeared with
            // the two-Options shape. Filling two halves independently was how a
            // user's `resume_subcommand` could get the built-in `--resume`
            // grafted on beside it.
            spec.resume = spec.resume.take().or(builtin.resume);
            spec.readonly_flag = spec.readonly_flag.take().or(builtin.readonly_flag);
            // UNION, not "or" — the only field here that merges rather than defers.
            // The built-in vars are an invariant of the backend, not a default the
            // user is picking between: they are the difference between a reviewer
            // confined to one tool and a reviewer handed whatever the environment
            // offers. A user listing a var of their own is ADDING to them, so
            // `readonly_env_unset = ["HTTP_PROXY"]` must not be read as consent to
            // stop clearing `OPENCODE_CONFIG`.
            for var in builtin.readonly_env_unset {
                if !spec.readonly_env_unset.contains(&var) {
                    spec.readonly_env_unset.push(var);
                }
            }
            // Union for the same reason, and with more at stake: dropping one of these
            // hands the repository under review the definition of the read-only agent.
            for path in builtin.readonly_displace {
                if !spec.readonly_displace.contains(&path) {
                    spec.readonly_displace.push(path);
                }
            }
            spec.workspace = spec.workspace.take().or(builtin.workspace);
            spec.system_prompt_flag = spec
                .system_prompt_flag
                .take()
                .or(builtin.system_prompt_flag);
            spec.model_flag = spec.model_flag.take().or(builtin.model_flag);
            spec.review_model = spec.review_model.take().or(builtin.review_model);
            spec.mcp = spec.mcp.take().or(builtin.mcp);
        } else {
            config.agents.insert(name, builtin);
        }
    }
    Ok(config)
}

impl Config {
    fn agent(&self, name: &str) -> io::Result<&AgentSpec> {
        self.agents.get(name).ok_or_else(|| {
            io::Error::other(format!("unknown agent '{name}': not in config agent map"))
        })
    }

    /// Select the backend for an automated review panel.
    ///
    /// An explicit `review_agent` is always honored. Otherwise Cursor's `agent`
    /// backend is preferred when its configured command is executable and its
    /// herdr integration is available; otherwise use the backend captured by the
    /// run (or `default_agent`).
    pub fn review_agent_for(
        &self,
        run_agent: Option<&str>,
        cursor_integration_available: bool,
    ) -> io::Result<String> {
        if let Some(name) = self.review_agent.as_deref() {
            self.agent(name)?;
            return Ok(name.to_owned());
        }

        if let Some(cursor) = self.agents.get("cursor")
            && cursor.readonly_flag.is_some()
            && command_available(&cursor.command)
            && cursor_integration_available
        {
            return Ok("cursor".into());
        }

        let name = run_agent.unwrap_or(&self.default_agent);
        self.agent(name)?;
        Ok(name.to_owned())
    }

    /// Project-relative paths to move aside before `agent` runs read-only. See
    /// [`AgentSpec::readonly_displace`] — validated at load, so every entry is
    /// relative and free of `..`.
    pub fn readonly_displace(&self, agent: &str) -> io::Result<&[String]> {
        Ok(&self.agent(agent)?.readonly_displace)
    }

    /// The resume surface `agent` offers, or `None` — either the config does not
    /// know this agent, or it knows it and it has none.
    ///
    /// **THE lookup, and the only one.** [`Config::resume_launch`] composes from
    /// it, and the review UI asks it whether a ⟳ promises the CONVERSATION back
    /// or merely a fresh agent reading the notes. The UI used to reach into
    /// `self.agents` and test `spec.resume` itself — a second classifier of one
    /// fact, which is the shape that has already cost this branch two rounds
    /// (`Capture::from_poll` vs `PaneState::from_poll`). Two callers, one
    /// answer.
    pub fn resume_surface(&self, agent: &str) -> Option<&ResumeSpec> {
        self.agents.get(agent)?.resume.as_ref()
    }

    /// How `agent` is handed an MCP server, if it can be. The caller needs this
    /// *before* [`Config::launch`]: the config file has to exist at
    /// [`McpDelivery::config_path`] by the time the agent starts.
    pub fn mcp_delivery(&self, agent: &str) -> io::Result<Option<&McpDelivery>> {
        Ok(self.agent(agent)?.mcp.as_ref())
    }

    /// Compose an agent launch command pinned to `project_dir`, together with the
    /// backend it was composed from.
    ///
    /// Returns an [`AgentLaunch`] rather than a bare `String` so the command and
    /// the backend name cannot be passed around separately and drift — see that
    /// type for the bug that motivated it.
    ///
    /// `mcp_config` is the path drovr wrote the server config to (see
    /// [`Config::mcp_delivery`]). How — or whether — that path reaches the agent
    /// depends on its [`McpDelivery`]: a flag backend gets it on the command
    /// line, a project-file backend only gets the flags that make it read the
    /// file it already knows about.
    pub fn launch(
        &self,
        agent: &str,
        project_dir: &str,
        readonly: bool,
        mcp_config: Option<&Path>,
    ) -> io::Result<AgentLaunch> {
        self.compose(agent, project_dir, readonly, mcp_config, None)
    }

    /// The one composer, shared by [`Config::launch`] and
    /// [`Config::resume_launch`] so a fresh launch and a resumed one cannot
    /// drift in their flags. `resume` decides only WHERE the id goes:
    /// a subcommand binds to the command and must precede every flag; a flag
    /// joins the flags.
    fn compose(
        &self,
        agent: &str,
        project_dir: &str,
        readonly: bool,
        mcp_config: Option<&Path>,
        resume: Option<(&ResumeSpec, &SessionId)>,
    ) -> io::Result<AgentLaunch> {
        let spec = self.agent(agent)?;
        // `env -u …` must precede the command word and a resume SUBCOMMAND must
        // follow it immediately, so the two orderings compose as
        // `env -u VAR <command> <subcommand> '<id>' …flags`.
        let mut command = String::new();
        if readonly && !spec.readonly_env_unset.is_empty() {
            command.push_str("env");
            for var in &spec.readonly_env_unset {
                command.push_str(" -u ");
                command.push_str(&shell_single_quote(var));
            }
            command.push(' ');
        }
        command.push_str(&spec.command);
        if let Some((resume @ ResumeSpec::Subcommand(_), session)) = resume {
            command.push(' ');
            command.push_str(resume.token());
            command.push(' ');
            command.push_str(&shell_single_quote(session.as_str()));
        }
        if readonly {
            let flag = spec.readonly_flag.as_ref().ok_or_else(|| {
                io::Error::other(format!(
                    "agent '{agent}' has no readonly_flag; cannot serve as reviewer"
                ))
            })?;
            command.push(' ');
            command.push_str(flag);
            if let Some(model) = &spec.review_model {
                let model_flag = spec.model_flag.as_ref().ok_or_else(|| {
                    io::Error::other(format!(
                        "agent '{agent}' has a review_model but no model_flag"
                    ))
                })?;
                command.push(' ');
                command.push_str(model_flag);
                command.push(' ');
                command.push_str(&shell_single_quote(model));
            }
        }
        if let Some(workspace) = &spec.workspace {
            if let WorkspaceArg::Flag { flag } = workspace {
                command.push(' ');
                command.push_str(flag);
            }
            command.push(' ');
            command.push_str(&shell_single_quote(project_dir));
        }
        if let Some(path) = mcp_config {
            let delivery = spec.mcp.as_ref().ok_or_else(|| {
                io::Error::other(format!(
                    "agent '{agent}' has no `mcp` mechanism; it cannot be given an \
                     MCP server (so it cannot serve on the review panel)"
                ))
            })?;
            if let McpDelivery::ConfigFlag { flag, .. } = delivery {
                command.push(' ');
                command.push_str(flag);
                command.push(' ');
                command.push_str(&shell_single_quote(&path.display().to_string()));
            }
            for flag in delivery.extra_flags() {
                command.push(' ');
                command.push_str(flag);
            }
        }
        if let Some(flag) = &spec.system_prompt_flag {
            let prompt = format!(
                "Your project root is {project_dir}. Treat it as the absolute workspace \
                 root: resolve every file path against it, and never read or edit files \
                 outside it. If this checkout is a git worktree that shares its .git with \
                 another checkout, ignore that outer checkout entirely — edits belong in \
                 {project_dir}."
            );
            command.push(' ');
            command.push_str(flag);
            command.push(' ');
            command.push_str(&shell_single_quote(&prompt));
        }
        // LAST, and always with its id in the same push. The flag's value is
        // OPTIONAL to the agent, so the id is what separates "resume this
        // conversation" from "open the session picker and park forever".
        if let Some((resume @ ResumeSpec::Flag(_), session)) = resume {
            command.push(' ');
            command.push_str(resume.token());
            command.push(' ');
            command.push_str(&shell_single_quote(session.as_str()));
        }
        Ok(AgentLaunch {
            backend: agent.to_owned(),
            command,
        })
    }

    /// Compose a launch that RESUMES `session` instead of starting a fresh
    /// conversation, or `Ok(None)` when `agent` offers no resume surface — in
    /// which case the caller must fall back to a plain [`Config::launch`] and
    /// re-seed the agent, because there is no way to ask this backend for its
    /// old session.
    ///
    /// `readonly` is re-passed exactly as it is for a fresh launch, and that is
    /// load-bearing for reviewers: a resumed reviewer launched without its
    /// `readonly_flag` is a second WRITER in a run that guarantees a single one.
    ///
    /// The id arrives as a [`SessionId`], whose alphabet
    /// (`[A-Za-z0-9._-]{1,128}`) is enforced at both of its constructors — so
    /// there is no "validate before composing" step to forget here, and no way
    /// to reach this function with an empty id and emit a bare flag.
    pub fn resume_launch(
        &self,
        target: &crate::run::ResumeTarget<'_>,
        project_dir: &str,
        readonly: bool,
    ) -> io::Result<Option<AgentLaunch>> {
        // The BACKEND comes out of the same bundle as the session, here, rather
        // than from two arguments a caller paired up. `AgentSession::resumable_for`
        // is the single chokepoint that ties a session id to the agent it means
        // anything to, and `ResumeTarget` is how that proof travels — taking the
        // pair apart at the one call site that composes `--resume` would hand it
        // straight back.
        // An agent the config does not know at all is an ERROR, not a reseed:
        // a phase recording a backend nothing defines is a broken config, and
        // `compose` below would fail on it anyway.
        self.agent(target.backend())?;
        // …but "known, and offers no way to resume" is an ordinary `Ok(None)`.
        // Read through `resume_surface`, the one lookup the review UI also asks.
        let Some(resume) = self.resume_surface(target.backend()) else {
            return Ok(None);
        };
        // No `mcp_config`, deliberately, and the consequence is ENFORCED
        // elsewhere rather than left as a warning here: a resumed agent is
        // handed no MCP server, so a resumed REVIEWER would have no
        // `submit_findings` tool and `delivered_review` would wait on a file
        // that can never appear. `NotRehydratable::Reviewer` is where that is
        // made unreachable — a reviewer is refused before it reaches this
        // function at all.
        //
        // Wiring the server through would need the task name and iteration to
        // rewrite the per-task MCP config for an OLD pass, which is the same
        // file a currently running panel's reviewers were launched against. A
        // panel is re-run, not rehydrated.
        self.compose(
            target.backend(),
            project_dir,
            readonly,
            None,
            Some((resume, target.session())),
        )
        .map(Some)
    }

    /// Return the composed reviewer launch command `"<command> <readonly_flag>"` for `agent`
    /// (defaults to `self.default_agent` when `agent` is `None`). Errors if the agent is
    /// unknown or has no `readonly_flag` ("cannot serve as reviewer").
    #[cfg(test)]
    pub fn reviewer_launch(&self, agent: Option<&str>) -> io::Result<String> {
        let name = agent.unwrap_or(&self.default_agent);
        let spec = self.agent(name)?;
        let flag = spec.readonly_flag.as_ref().ok_or_else(|| {
            io::Error::other(format!(
                "agent '{name}' has no readonly_flag; cannot serve as reviewer"
            ))
        })?;
        Ok(format!("{} {}", spec.command, flag))
    }
}

/// A composed agent invocation, inseparable from the backend that composed it.
///
/// The two travel together because they are one fact — "this pane runs THIS
/// agent, invoked THIS way" — and splitting them into two `&str` parameters put
/// a real bug in the tree: a caller passed a literal `"claude"` alongside a
/// command composed for a different backend, and the phase then recorded a
/// backend its pane was not running. Session capture checks a pane's session
/// against the recorded backend, so the mismatch silently captured nothing, for
/// exactly the panes (reviewers) whose session cannot be re-read later.
///
/// Only [`Config::launch`] constructs one, so the backend is always the name the
/// command was actually built from. There is no constructor that lets a caller
/// supply them independently.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentLaunch {
    backend: String,
    command: String,
}

impl AgentLaunch {
    /// The agent name (`claude`, `cursor`, …) this launch runs.
    pub fn backend(&self) -> &str {
        &self.backend
    }
    /// The full shell invocation to run in the pane.
    pub fn command(&self) -> &str {
        &self.command
    }
    /// Test-only: build a launch without a `Config`. The pairing this type
    /// exists to guarantee is a property of `Config::launch`, and a test that
    /// deliberately wants a cursor launch has no config to compose one from.
    #[cfg(test)]
    pub fn for_test(backend: &str, command: &str) -> AgentLaunch {
        AgentLaunch {
            backend: backend.to_owned(),
            command: command.to_owned(),
        }
    }
}

/// Reject agent commands that would resolve against an untrusted working
/// directory. drovr often runs with its CWD inside the repository under review;
/// a relative-path command (`./git`, `../tool`, `bin/agent`) resolves against
/// that CWD, so a hostile repo (or a tampered config) could drop a lookalike
/// binary and have drovr execute it. A bare name (`claude`) is resolved via
/// `$PATH` — a trusted, user-controlled search path — and an absolute path names
/// exactly one file; both are allowed. Anything relative-but-pathful is not.
fn validate_command(command: &str) -> io::Result<()> {
    if command.trim().is_empty() {
        return Err(io::Error::other("agent command must not be empty"));
    }
    // Validate exactly what gets spawned. Surrounding whitespace is never a
    // valid command and would otherwise pass the path check below (a single
    // component) only to fail later with a cryptic spawn error — reject it here.
    if command != command.trim() {
        return Err(io::Error::other(format!(
            "agent command '{command}' has leading or trailing whitespace"
        )));
    }
    let path = std::path::Path::new(command);
    if path.components().count() > 1 && !path.is_absolute() {
        return Err(io::Error::other(format!(
            "agent command '{command}' is a relative path; use a bare name \
             (resolved via $PATH) or an absolute path"
        )));
    }
    Ok(())
}

/// Reject an MCP mechanism whose file would land outside the directory it is
/// meant to. A `ProjectFile` path is joined onto the project dir, so an absolute
/// path or a `..` component would have drovr write the server config anywhere on
/// disk — under a name a hostile config chooses.
fn validate_delivery(delivery: &McpDelivery) -> io::Result<()> {
    let Some(path) = delivery.project_relative_path() else {
        return Ok(());
    };
    validate_project_relative(path, "mcp `path`")
}

/// Reject a project-relative path that would land outside the project. Shared by
/// every config key naming one, because they are all joined onto the project dir and
/// an absolute or `..`-bearing value would send drovr's write — or, for
/// [`AgentSpec::readonly_displace`], drovr's **rename** — anywhere on disk under a
/// name a hostile config chooses. Displacement makes this sharper than it was: the
/// mcp path only ever writes a file drovr owns, while a displace path MOVES whatever
/// it names.
/// Every component must be a plain name. Rejecting only absolute paths and `..`
/// leaves `.` — which is relative, contains no `..`, and resolves to the project
/// *root*. For an mcp `path` that merely fails later when drovr tries to write a file
/// over a directory; for a displace entry it means `rename`-ing the entire checkout
/// aside before a reviewer spawns. One rule ("a path made of names") covers the
/// absolute, the traversing and the root-resolving cases at once.
fn validate_project_relative(path: &str, what: &str) -> io::Result<()> {
    if path.trim().is_empty() {
        return Err(io::Error::other(format!("{what} must not be empty")));
    }
    let mut components = Path::new(path).components().peekable();
    if components.peek().is_none()
        || components.any(|c| !matches!(c, std::path::Component::Normal(_)))
    {
        return Err(io::Error::other(format!(
            "{what} '{path}' must be a relative path inside the project, \
             made only of ordinary path names"
        )));
    }
    Ok(())
}

fn command_available(command: &str) -> bool {
    let path = std::path::Path::new(command);
    if path.components().count() > 1 {
        return executable_file(path);
    }

    crate::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).any(|dir| executable_file(&dir.join(command))))
        .unwrap_or(false)
}

fn executable_file(path: &std::path::Path) -> bool {
    let Ok(metadata) = path.metadata() else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::ENV_LOCK;

    // Sets XDG_CONFIG_HOME to `dir`. Caller must hold ENV_LOCK.
    fn set_config_home(dir: &std::path::Path) {
        unsafe {
            std::env::set_var("XDG_CONFIG_HOME", dir);
        }
    }

    /// The bare-`#[serde(default)]`-on-bool trap, for the opt-OUT switch: a
    /// config file that says nothing about reaping must still reap, and one that
    /// says `false` must be believed. Written as its own test because the
    /// failure mode is silent in both directions — a default of `false` turns
    /// reaping off for every user with a config file, and ignoring an explicit
    /// `false` closes panes for the one user who asked drovr not to.
    #[test]
    fn reaping_is_on_unless_a_config_file_turns_it_off() {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        set_config_home(tmp.path());
        let path = tmp.path().join("drovr/config.toml");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();

        assert!(
            Config::default().reap_finished_panes,
            "the built-in default is on"
        );
        // A real config file that simply does not mention it.
        std::fs::write(&path, "default_agent = \"claude\"\n").unwrap();
        assert!(
            load_config().unwrap().reap_finished_panes,
            "an absent key must not read as `false`"
        );
        std::fs::write(&path, "reap_finished_panes = false\n").unwrap();
        assert!(
            !load_config().unwrap().reap_finished_panes,
            "an explicit opt-out must be honoured"
        );
    }

    #[test]
    fn absent_file_yields_defaults() {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        set_config_home(tmp.path());

        let cfg = load_config().unwrap();
        assert_eq!(cfg, Config::default());
        assert_eq!(cfg.default_agent, "claude");
        assert_eq!(cfg.review_agent, None);
        assert_eq!(
            cfg.angles,
            vec!["correctness", "security", "error-handling", "type-design"]
        );
        assert_eq!(cfg.serve_host, "127.0.0.1");
        // Reflex defaults: on, no preamble override, no section overrides.
        assert!(cfg.reflex.enabled);
        assert_eq!(cfg.reflex.preamble, None);
        assert!(cfg.reflex.sections.is_empty());
        assert_eq!(
            cfg.reviewer_launch(None).unwrap(),
            "claude --permission-mode plan"
        );
        assert!(cfg.agents.contains_key("cursor"));
        assert_eq!(
            cfg.launch("cursor", "/tmp/my worktree", false, None)
                .unwrap()
                .command(),
            "agent --workspace '/tmp/my worktree'"
        );
        assert_eq!(
            cfg.launch("cursor", "/tmp/my worktree", true, None)
                .unwrap()
                .command(),
            "agent --mode plan --model 'composer-2.5' --workspace '/tmp/my worktree'"
        );
    }

    /// Replacing `opencode.json` is what keeps a repository under review from handing
    /// its own MCP servers to a read-only reviewer — but only if that file is the last
    /// word on the subject. `OPENCODE_CONFIG` names another config file and *merges*
    /// it (probed: a project file and an `OPENCODE_CONFIG` file resolve to BOTH their
    /// servers), so a value inherited from the launching environment reopens exactly
    /// the hole the replacement closes. The reviewer launch has to clear it.
    #[test]
    fn a_readonly_opencode_launch_clears_the_config_var_that_would_merge_more_servers() {
        let cfg = Config::default();
        let launch = cfg.launch("opencode", "/tmp/proj", true, None).unwrap();
        let readonly = launch.command();
        assert!(
            readonly.starts_with("env -u ") && readonly.contains(" opencode --agent plan "),
            "a read-only reviewer must not inherit a second config file: {readonly}"
        );
        // Writer phases keep it: drovr does not replace `opencode.json` for them, so
        // clearing the var would buy no guarantee and would break a user who points
        // it at their real provider config.
        assert_eq!(
            cfg.launch("opencode", "/tmp/proj", false, None)
                .unwrap()
                .command(),
            "opencode '/tmp/proj'"
        );
    }

    /// `OPENCODE_CONFIG` is not the only way in. Probed against opencode 1.18.3, each
    /// of these loads config *on top of* the project `opencode.json` drovr replaced:
    /// `OPENCODE_CONFIG_CONTENT` (inline JSON) and `OPENCODE_CONFIG_DIR` both resolve
    /// to their server ALONGSIDE `drovr-findings`, and `OPENCODE_PERMISSION` sets the
    /// permission block wholesale. Clearing one of four doors is not a guard.
    #[test]
    fn every_opencode_config_door_is_shut_for_a_reviewer_not_just_the_first_one() {
        let cfg = Config::default();
        let launch = cfg.launch("opencode", "/tmp/proj", true, None).unwrap();
        let readonly = launch.command();
        for var in [
            "OPENCODE_CONFIG",
            "OPENCODE_CONFIG_CONTENT",
            "OPENCODE_CONFIG_DIR",
            "OPENCODE_PERMISSION",
        ] {
            assert!(
                readonly.contains(&format!("-u '{var}'")),
                "{var} still reaches the reviewer: {readonly}"
            );
        }
    }

    /// The built-in vars are an invariant of the opencode backend, not a default the
    /// user is choosing between. Redefining the agent to clear a var of their own must
    /// ADD to them — taking the override as the complete set silently reopens the hole
    /// the field exists to close.
    #[test]
    fn a_user_env_unset_list_adds_to_the_built_in_one_rather_than_replacing_it() {
        let _lock = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        set_config_home(dir.path());
        std::fs::create_dir_all(dir.path().join("drovr")).unwrap();
        std::fs::write(
            dir.path().join("drovr/config.toml"),
            "[agents.opencode]\ncommand = \"opencode\"\n\
             readonly_env_unset = [\"HTTP_PROXY\"]\n",
        )
        .unwrap();

        let cfg = load_config().unwrap();
        let spec = &cfg.agents["opencode"];
        assert!(
            spec.readonly_env_unset.iter().any(|v| v == "HTTP_PROXY"),
            "the user's own var must survive: {:?}",
            spec.readonly_env_unset
        );
        assert!(
            spec.readonly_env_unset
                .iter()
                .any(|v| v == "OPENCODE_CONFIG"),
            "the built-in guard must survive a partial override: {:?}",
            spec.readonly_env_unset
        );
    }

    /// `schema` decides the shape of a document written into someone's project, and
    /// `load_config` replaces an overridden `mcp` table wholesale. A default therefore
    /// means an `[agents.opencode.mcp]` stanza that only retunes the path quietly
    /// starts writing cursor's schema into `opencode.json` — a file opencode parses
    /// without error and reads no servers from. It has to be stated.
    #[test]
    fn an_mcp_override_must_state_its_schema_rather_than_inherit_a_default() {
        let _lock = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        set_config_home(dir.path());
        std::fs::create_dir_all(dir.path().join("drovr")).unwrap();
        std::fs::write(
            dir.path().join("drovr/config.toml"),
            "[agents.opencode]\ncommand = \"opencode\"\n\
             [agents.opencode.mcp]\nmechanism = \"project-file\"\npath = \"custom.json\"\n",
        )
        .unwrap();

        load_config().expect_err("an mcp table without a schema must fail the load");
    }

    /// opencode names the project as a POSITIONAL (`opencode <dir>`), not behind a
    /// flag — and its argument parser ignores unknown options silently, so a
    /// made-up `--dir` would compose a command that looks pinned and is not.
    /// The dir therefore has to arrive as a bare quoted word.
    #[test]
    fn the_opencode_workspace_is_a_positional_not_a_flag() {
        let cfg = Config::default();
        assert_eq!(
            cfg.launch("opencode", "/tmp/my worktree", false, None)
                .unwrap()
                .command(),
            "opencode '/tmp/my worktree'"
        );
        // Read-only adds the config-discovery unsets in front (see
        // `every_opencode_config_door_is_shut_for_a_reviewer_not_just_the_first_one`);
        // what matters here is that the dir stays a bare trailing word either way.
        let launch = cfg
            .launch("opencode", "/tmp/my worktree", true, None)
            .unwrap();
        let readonly = launch.command();
        assert!(
            readonly.ends_with("opencode --agent plan '/tmp/my worktree'"),
            "{readonly}"
        );
    }

    /// `readonly_displace` entries are *renamed*, so the check that they stay inside
    /// the project is not enough — they must also name something *within* it. A lone
    /// `.` is relative and has no `..`, yet `project_dir.join(".")` is the checkout
    /// root: drovr would move the entire repository aside before spawning a reviewer.
    #[test]
    fn a_displace_entry_that_resolves_to_the_project_root_is_rejected() {
        let _lock = ENV_LOCK.lock().unwrap();
        for bad in [".", "./.", "./"] {
            let dir = tempfile::tempdir().unwrap();
            set_config_home(dir.path());
            std::fs::create_dir_all(dir.path().join("drovr")).unwrap();
            std::fs::write(
                dir.path().join("drovr/config.toml"),
                format!("[agents.tool]\ncommand = \"tool\"\nreadonly_displace = [{bad:?}]\n"),
            )
            .unwrap();

            let err = load_config()
                .expect_err(&format!("'{bad}' resolves to the project root"))
                .to_string();
            assert!(err.contains("tool"), "the error must name the agent: {err}");
        }
    }

    /// The spelling README documents, both arms of it. A user-defined backend has
    /// to be able to say "positional" — that is the whole reason the flag string
    /// became a mechanism — and the doc is only worth as much as this test.
    #[test]
    fn both_workspace_mechanisms_round_trip_through_the_config_file() {
        let _lock = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        set_config_home(dir.path());
        std::fs::create_dir_all(dir.path().join("drovr")).unwrap();
        std::fs::write(
            dir.path().join("drovr/config.toml"),
            "[agents.flagtool]\ncommand = \"flagtool\"\n\
             [agents.flagtool.workspace]\nmechanism = \"flag\"\nflag = \"--root\"\n\
             [agents.postool]\ncommand = \"postool\"\n\
             [agents.postool.workspace]\nmechanism = \"positional\"\n",
        )
        .unwrap();

        let cfg = load_config().unwrap();
        assert_eq!(
            cfg.launch("flagtool", "/tmp/proj", false, None)
                .unwrap()
                .command(),
            "flagtool --root '/tmp/proj'"
        );
        assert_eq!(
            cfg.launch("postool", "/tmp/proj", false, None)
                .unwrap()
                .command(),
            "postool '/tmp/proj'"
        );
    }

    /// The failure mode `deny_unknown_fields` exists for: a config written against
    /// the old `workspace_flag = "…"` spelling must not load clean and leave the
    /// agent unpinned. Every key here changes what drovr spawns, so an unrecognised
    /// one is a config error, not a comment.
    #[test]
    fn a_stale_agent_key_fails_the_load_instead_of_being_ignored() {
        let _lock = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        set_config_home(dir.path());
        std::fs::create_dir_all(dir.path().join("drovr")).unwrap();
        std::fs::write(
            dir.path().join("drovr/config.toml"),
            "[agents.claude]\ncommand = \"claude\"\nworkspace_flag = \"--add-dir\"\n",
        )
        .unwrap();

        let err = load_config().expect_err("an unknown agent key must fail the load");
        assert!(
            err.to_string().contains("workspace_flag"),
            "the error must name the offending key: {err}"
        );
    }

    /// The same rule has to hold one level down. `schema` is what decides the *shape*
    /// of the document drovr writes for a backend, and it has a default — so a typo
    /// there does not fail, it silently selects the other backend's schema and the
    /// reviewer is handed a config it cannot read.
    #[test]
    fn a_stale_key_inside_a_mechanism_table_fails_the_load_too() {
        let _lock = ENV_LOCK.lock().unwrap();
        for (table, body) in [
            (
                "mcp",
                "[agents.tool.mcp]\nmechanism = \"project-file\"\npath = \"x.json\"\nschemas = \"opencode\"\n",
            ),
            (
                "workspace",
                "[agents.tool.workspace]\nmechanism = \"flag\"\nflagg = \"--root\"\n",
            ),
        ] {
            let dir = tempfile::tempdir().unwrap();
            set_config_home(dir.path());
            std::fs::create_dir_all(dir.path().join("drovr")).unwrap();
            std::fs::write(
                dir.path().join("drovr/config.toml"),
                format!("[agents.tool]\ncommand = \"tool\"\n{body}"),
            )
            .unwrap();

            load_config().expect_err(&format!("an unknown key in [{table}] must fail the load"));
        }
    }

    #[test]
    fn a_launch_carries_the_backend_it_was_composed_from() {
        // The command and the backend name are one fact, and splitting them into
        // two arguments put a real bug in the tree: a caller passed a literal
        // "claude" beside a command composed for another agent, so the phase
        // recorded a backend its pane was not running and session capture — which
        // checks a session against that backend — silently recorded nothing.
        //
        // `Config::launch` is the only constructor, so the pair cannot disagree:
        // the backend is always the name the command was built from. Note the
        // command here does not contain the string "cursor" anywhere — nothing
        // downstream could recover the backend by inspecting it.
        let cfg = Config::default();
        let launch = cfg.launch("cursor", "/tmp/p", true, None).unwrap();
        assert_eq!(launch.backend(), "cursor");
        assert!(
            launch.command().starts_with("agent "),
            "{}",
            launch.command()
        );
        assert!(
            !launch.command().contains("cursor"),
            "the backend is NOT recoverable from the command: {}",
            launch.command()
        );

        let claude = cfg.launch("claude", "/tmp/p", false, None).unwrap();
        assert_eq!(claude.backend(), "claude");
        assert!(claude.command().starts_with("claude"));
    }

    fn sid(value: &str) -> SessionId {
        SessionId::new(value.to_owned()).expect("test session id must be well-formed")
    }

    /// A phase carrying a resume target, built the way production builds one.
    /// There is deliberately no shortcut constructor for `ResumeTarget`: the
    /// bundle exists precisely so a session id and a backend cannot be paired
    /// up by hand, and a test-only back door would be the first thing to
    /// re-open that.
    fn resumable_phase(backend: &str, session: &str) -> crate::run::Phase {
        let mut p = crate::run::Phase::new("t");
        p.record_launch(backend, None);
        assert!(p.record_session(sid(session)), "fixture must attach");
        p
    }

    #[test]
    fn claude_and_cursor_resume_with_a_flag_and_codex_with_nothing() {
        let cfg = Config::default();
        assert_eq!(
            cfg.agents["claude"].resume,
            Some(builtin_resume_flag())
        );
        assert_eq!(
            cfg.agents["cursor"].resume,
            Some(builtin_resume_flag())
        );
        // codex gets NEITHER on purpose: `codex resume <id>` is the documented
        // shape but was never verified against the real CLI, and an unverified
        // guess composes a wrong command line where `None` merely reseeds.
        assert_eq!(cfg.agents["codex"].resume, None);
    }

    #[test]
    fn an_empty_resume_token_is_not_constructible() {
        // An empty flag is precisely how a bare `--resume` gets emitted, and a
        // bare `--resume` opens claude's interactive picker and parks the pane
        // forever. The guard used to live in `TryFrom<AgentSpecWire>`, which
        // covers the config file and NOTHING else: `ResumeSpec::Flag("".into())`
        // was constructible in code, and the built-in map and every future
        // caller were on the honour system. It lives in the type now.
        assert!(ResumeSpec::flag("").is_err());
        assert!(ResumeSpec::flag("   ").is_err());
        assert!(ResumeSpec::flag("\t\n").is_err());
        assert!(ResumeSpec::subcommand("").is_err());
        assert!(ResumeSpec::subcommand(" ").is_err());
        // And the valid one still round-trips to exactly what it was given.
        assert_eq!(ResumeSpec::flag("--resume").unwrap().token(), "--resume");
        assert_eq!(ResumeSpec::subcommand("resume").unwrap().token(), "resume");
    }

    #[test]
    fn a_flag_resume_carries_its_id_and_never_appears_bare() {
        let cfg = Config::default();
        let ph = resumable_phase("claude", "abc-123.def_4");
        let launch = cfg
            .resume_launch(&ph.resume_target().unwrap(), "/tmp/p", false)
            .unwrap()
            .expect("claude offers a resume flag");
        assert_eq!(launch.backend(), "claude");
        assert!(
            launch.command().contains("--resume 'abc-123.def_4'"),
            "{}",
            launch.command()
        );
        // A bare `--resume` opens claude's interactive session picker, which
        // parks the pane forever. The id is never optional here.
        assert!(
            !launch.command().ends_with("--resume"),
            "never a bare flag: {}",
            launch.command()
        );
        assert!(
            !launch.command().contains("--resume  "),
            "never an empty id: {}",
            launch.command()
        );
        // Same project pinning a fresh launch gets: a session resolves under
        // `<profile>/projects/<escaped-cwd>/`, so the cwd must not drift.
        assert!(launch.command().contains("--add-dir '/tmp/p'"), "{}", launch.command());
    }

    #[test]
    fn a_resumed_reviewer_still_carries_its_readonly_flag() {
        // A resumed reviewer without its read-only flag is a second WRITER in a
        // run built on single-writer discipline.
        let cfg = Config::default();
        let ph = resumable_phase("claude", "sess-1");
        let launch = cfg
            .resume_launch(&ph.resume_target().unwrap(), "/tmp/p", true)
            .unwrap()
            .unwrap();
        assert!(
            launch.command().contains("--permission-mode plan"),
            "{}",
            launch.command()
        );
        assert!(launch.command().contains("--resume 'sess-1'"), "{}", launch.command());
    }

    #[test]
    fn a_resume_subcommand_comes_immediately_after_the_command() {
        let mut cfg = Config::default();
        let codex = cfg.agents.get_mut("codex").unwrap();
        codex.resume = Some(ResumeSpec::subcommand("resume").expect("literal is non-empty"));
        let ph = resumable_phase("codex", "sess-9");
        let launch = cfg
            .resume_launch(&ph.resume_target().unwrap(), "/tmp/p", false)
            .unwrap()
            .unwrap();
        // A subcommand is not a flag: it binds to the command and must precede
        // every flag, so the ORDER is the assertion, not mere presence.
        assert!(
            launch.command().starts_with("codex resume 'sess-9' "),
            "{}",
            launch.command()
        );
        assert!(launch.command().contains("-C '/tmp/p'"), "{}", launch.command());
    }

    #[test]
    fn an_agent_with_no_resume_surface_asks_the_caller_to_reseed() {
        // `Ok(None)`, not an error: "this backend cannot be resumed" is a normal
        // outcome that rehydrate answers with a fresh launch plus a re-seed.
        let cfg = Config::default();
        let codex = resumable_phase("codex", "sess-1");
        assert!(
            cfg.resume_launch(&codex.resume_target().unwrap(), "/tmp/p", false)
                .unwrap()
                .is_none()
        );
        // An unknown agent is still an error — and the backend comes out of the
        // bundle, so this cannot be tested by passing a mismatched pair.
        let nope = resumable_phase("nope", "s");
        assert!(
            cfg.resume_launch(&nope.resume_target().unwrap(), "/tmp/p", false)
                .is_err()
        );
    }

    #[test]
    fn an_explicit_agent_block_keeps_the_builtin_resume_flag() {
        // The documented merge trap: a user config with an explicit
        // `[agents.claude]` block must not silently drop fields it omits.
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("drovr");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("config.toml"),
            "[agents.claude]\ncommand = \"claude\"\n",
        )
        .unwrap();
        set_config_home(tmp.path());

        let cfg = load_config().unwrap();
        assert_eq!(
            cfg.agents["claude"].resume,
            Some(builtin_resume_flag())
        );
    }

    #[test]
    fn an_explicit_resume_subcommand_is_not_joined_by_the_builtin_flag() {
        // The resume surface merges as ONE unit. Filling each field
        // independently would graft the built-in `--resume` onto an agent the
        // user deliberately gave a subcommand, producing a spec with both —
        // i.e. an ambiguous, unusable resume.
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("drovr");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("config.toml"),
            "[agents.claude]\ncommand = \"claude\"\nresume_subcommand = \"resume\"\n",
        )
        .unwrap();
        set_config_home(tmp.path());

        let cfg = load_config().unwrap();
        assert_eq!(
            cfg.agents["claude"].resume,
            Some(ResumeSpec::subcommand("resume").expect("literal is non-empty"))
        );
    }

    #[test]
    fn an_agent_entry_with_only_a_command_still_loads() {
        // `AgentSpec` is now built through `AgentSpecWire`, and a field that
        // lost its `#[serde(default)]` in that move would make every real
        // user config fail to load — `load_config` returning Err is a hard
        // stop, not a degradation. Pin the minimal entry.
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("drovr");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("config.toml"),
            "[agents.minimal]\ncommand = \"minimal\"\n",
        )
        .unwrap();
        set_config_home(tmp.path());

        let cfg = load_config().expect("an entry with only `command` must load");
        let spec = &cfg.agents["minimal"];
        assert_eq!(spec.command, "minimal");
        // Every optional field absent, and no resume surface invented for it.
        assert_eq!(spec.readonly_flag, None);
        assert_eq!(spec.workspace, None);
        // No read-only guards invented for an agent that asked for none either —
        // these are per-backend facts, not defaults.
        assert!(spec.readonly_env_unset.is_empty());
        assert!(spec.readonly_displace.is_empty());
        assert_eq!(spec.system_prompt_flag, None);
        assert_eq!(spec.model_flag, None);
        assert_eq!(spec.review_model, None);
        assert_eq!(spec.resume, None);
        // …and the built-ins are still whole beside it.
        assert_eq!(
            cfg.agents["claude"].resume,
            Some(builtin_resume_flag())
        );
    }

    #[test]
    fn an_agent_claiming_both_resume_shapes_is_rejected() {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("drovr");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("config.toml"),
            "[agents.claude]\ncommand = \"claude\"\nresume_flag = \"--resume\"\n\
             resume_subcommand = \"resume\"\n",
        )
        .unwrap();
        set_config_home(tmp.path());

        let err = load_config().expect_err("both resume shapes must be rejected");
        let msg = err.to_string();
        assert!(msg.contains("claude"), "error should name the agent: {msg}");
        assert!(
            msg.contains("resume_flag") && msg.contains("resume_subcommand"),
            "error should name both keys: {msg}"
        );

        // An empty flag is the bare-`--resume` hazard written into a config
        // file, and it is rejected at load rather than composed.
        std::fs::write(
            dir.join("config.toml"),
            "[agents.claude]\ncommand = \"claude\"\nresume_flag = \"\"\n",
        )
        .unwrap();
        assert!(load_config().is_err(), "an empty resume_flag must be rejected");

        // BOTH keys are checked, not just the first. An empty subcommand
        // composes `<command>  '<id>'` — the id as a positional argument —
        // which is its own kind of wrong.
        std::fs::write(
            dir.join("config.toml"),
            "[agents.codex]\ncommand = \"codex\"\nresume_subcommand = \"   \"\n",
        )
        .unwrap();
        let err = load_config().expect_err("an empty resume_subcommand must be rejected");
        assert!(
            err.to_string().contains("resume_subcommand"),
            "the error must name the key at fault: {err}"
        );
    }

    #[test]
    fn detects_cursor_and_honors_explicit_override() {
        let _lock = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::remove_var("DROVR_AGENT");
            std::env::set_var("CURSOR_AGENT", "1");
        }
        assert_eq!(invoking_agent(&Config::default()), "cursor");
        unsafe {
            std::env::set_var("DROVR_AGENT", "custom");
        }
        assert_eq!(invoking_agent(&Config::default()), "custom");
        unsafe {
            std::env::remove_var("DROVR_AGENT");
            std::env::remove_var("CURSOR_AGENT");
            std::env::remove_var("CLAUDECODE");
            std::env::set_var("CODEX_THREAD_ID", "thread");
        }
        assert_eq!(invoking_agent(&Config::default()), "codex");
        assert!(Config::default().agents.contains_key("codex"));
        unsafe {
            std::env::remove_var("CODEX_THREAD_ID");
        }
    }

    /// claude carries the server config on its command line, so the path drovr
    /// wrote must appear there — with `--strict-mcp-config`, so the reviewer gets
    /// drovr's one tool and nothing the user happens to have configured globally.
    #[test]
    fn a_config_flag_backend_takes_the_path_on_its_command_line() {
        let cfg = Config::default();
        let path = std::path::Path::new("/data/runs/r/task-1-review-mcp.json");
        let cmd = cfg.launch("claude", "/tmp/proj", true, Some(path)).unwrap().command().to_owned();
        assert!(
            cmd.contains("--mcp-config '/data/runs/r/task-1-review-mcp.json'"),
            "{cmd}"
        );
        assert!(cmd.contains("--strict-mcp-config"), "{cmd}");
    }

    /// cursor has no such flag: it only reads `.cursor/mcp.json` inside the
    /// project. Putting the path on its command line would be a flag it rejects,
    /// so the launch carries only what makes it trust that file.
    #[test]
    fn a_project_file_backend_gets_flags_but_never_the_path() {
        let cfg = Config::default();
        let path = std::path::Path::new("/tmp/proj/.cursor/mcp.json");
        let cmd = cfg.launch("cursor", "/tmp/proj", true, Some(path)).unwrap().command().to_owned();
        assert!(cmd.contains("--approve-mcps"), "{cmd}");
        assert!(
            !cmd.contains("mcp.json"),
            "cursor has no flag that can carry the path: {cmd}"
        );
    }

    /// claude's plan mode gates tool use, so the one tool the panel depends on is
    /// pre-allowed by name — and only that one. The name is derived from the server
    /// key drovr registers, so the allowlist cannot drift from what is served.
    #[test]
    fn the_claude_reviewer_launch_pre_allows_exactly_the_findings_tool() {
        let cfg = Config::default();
        let cmd = cfg
            .launch(
                "claude",
                "/tmp/proj",
                true,
                Some(std::path::Path::new("/x.json")),
            )
            .unwrap()
            .command()
            .to_owned();
        let tool = crate::mcp_findings::qualified_tool_name();
        assert!(cmd.contains(&format!("--allowedTools={tool}")), "{cmd}");
        assert!(
            !cmd.contains(&format!("--allowedTools {tool}")),
            "the flag is variadic, so it must use the `=` form or it swallows the \
             argument after it: {cmd}"
        );
        assert_eq!(
            cmd.matches("--allowedTools").count(),
            1,
            "exactly one carve-out, not a list that could grow: {cmd}"
        );
    }

    #[test]
    fn no_mcp_config_leaves_the_launch_untouched() {
        let cfg = Config::default();
        let cmd = cfg.launch("claude", "/tmp/proj", true, None).unwrap().command().to_owned();
        assert!(!cmd.contains("--mcp-config"), "{cmd}");
        assert!(!cmd.contains("--strict-mcp-config"), "{cmd}");
    }

    /// A backend with no MCP mechanism cannot be handed one. Failing here beats
    /// launching a reviewer that has no way to deliver findings.
    #[test]
    fn a_backend_without_a_mechanism_refuses_an_mcp_config() {
        let cfg = Config::default();
        assert!(cfg.mcp_delivery("codex").unwrap().is_none());
        let err = cfg
            .launch(
                "codex",
                "/tmp/proj",
                true,
                Some(std::path::Path::new("/tmp/x.json")),
            )
            .expect_err("codex has no MCP mechanism");
        assert!(err.to_string().contains("codex"), "{err}");
    }

    /// The two mechanisms put the file in different places: drovr's own run dir
    /// for a flag backend, inside the project for one that only reads a fixed path.
    #[test]
    fn each_mechanism_places_its_config_where_that_backend_looks() {
        let cfg = Config::default();
        let run_dir = std::path::Path::new("/data/runs/r");
        let project = std::path::Path::new("/tmp/proj");
        assert_eq!(
            cfg.mcp_delivery("claude")
                .unwrap()
                .unwrap()
                .config_path(run_dir, project, "task-1"),
            run_dir.join("task-1-review-mcp.json")
        );
        let cursor = cfg.mcp_delivery("cursor").unwrap().unwrap();
        assert_eq!(
            cursor.config_path(run_dir, project, "task-1"),
            project.join(".cursor/mcp.json")
        );
        assert_eq!(cursor.project_relative_path(), Some(".cursor/mcp.json"));
        assert_eq!(
            cfg.mcp_delivery("claude")
                .unwrap()
                .unwrap()
                .project_relative_path(),
            None,
            "a flag backend's config never lands in the project"
        );
    }

    #[test]
    fn mcp_mechanisms_parse_from_config_and_are_merged_per_agent() {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("drovr");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("config.toml"),
            r#"
[agents.mine]
command = "mine"
readonly_flag = "--ro"

[agents.mine.mcp]
mechanism = "config-flag"
flag = "--servers"
extra_flags = ["--only-these"]
schema = "mcp-servers"

[agents.cursor]
command = "agent"
"#,
        )
        .unwrap();
        set_config_home(tmp.path());

        let cfg = load_config().unwrap();
        let cmd = cfg
            .launch(
                "mine",
                "/tmp/p",
                true,
                Some(std::path::Path::new("/tmp/s.json")),
            )
            .unwrap()
            .command()
            .to_owned();
        assert!(cmd.contains("--servers '/tmp/s.json'"), "{cmd}");
        assert!(cmd.contains("--only-these"), "{cmd}");
        // An agent that overrides a built-in without restating `mcp` keeps it.
        assert_eq!(
            cfg.mcp_delivery("cursor")
                .unwrap()
                .unwrap()
                .project_relative_path(),
            Some(".cursor/mcp.json")
        );
    }

    /// The project-relative path is joined onto the project dir, so an absolute
    /// or traversing value would write outside the project. Reject it at load.
    #[test]
    fn a_project_file_path_that_escapes_the_project_is_rejected() {
        let _lock = ENV_LOCK.lock().unwrap();
        for bad in ["/etc/mcp.json", "../../.cursor/mcp.json"] {
            let tmp = tempfile::tempdir().unwrap();
            let dir = tmp.path().join("drovr");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                dir.join("config.toml"),
                format!(
                    "[agents.mine]\ncommand = \"mine\"\n\n[agents.mine.mcp]\n\
                     mechanism = \"project-file\"\npath = {bad:?}\n\
                     schema = \"mcp-servers\"\n"
                ),
            )
            .unwrap();
            set_config_home(tmp.path());
            let err = load_config().expect_err("path '{bad}' must be rejected");
            assert!(err.to_string().contains("mine"), "{err}");
        }
    }

    #[test]
    fn validate_command_accepts_bare_and_absolute() {
        assert!(validate_command("claude").is_ok());
        assert!(validate_command("cursor-agent").is_ok());
        assert!(validate_command("/usr/local/bin/claude").is_ok());
    }

    #[test]
    fn validate_command_rejects_relative_and_empty() {
        assert!(validate_command("./claude").is_err());
        assert!(validate_command("../bin/claude").is_err());
        assert!(validate_command("bin/agent").is_err());
        assert!(validate_command("").is_err());
        assert!(validate_command("   ").is_err());
        // Surrounding whitespace: rejected clearly rather than passing here and
        // failing later at spawn time.
        assert!(validate_command(" claude").is_err());
        assert!(validate_command("claude ").is_err());
    }

    #[test]
    fn load_config_rejects_relative_command() {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("drovr");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("config.toml"),
            r#"
default_agent = "evil"

[agents.evil]
command = "./git"
"#,
        )
        .unwrap();
        set_config_home(tmp.path());

        let err = load_config().expect_err("relative-path command must be rejected");
        let msg = err.to_string();
        assert!(msg.contains("evil"), "error should name the agent: {msg}");
        assert!(
            msg.contains("relative path"),
            "error should explain the reason: {msg}"
        );
    }

    #[test]
    fn parses_spec_example_toml() {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("drovr");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("config.toml"),
            r#"
default_agent = "claude"
review_agent = "codex"
serve_host = "0.0.0.0"
angles = ["correctness", "security", "error-handling", "type-design"]

[agents.claude]
command = "claude"
readonly_flag = "--permission-mode plan"

[agents.codex]
command = "codex"
readonly_flag = "--sandbox read-only"
"#,
        )
        .unwrap();
        set_config_home(tmp.path());

        let cfg = load_config().unwrap();
        assert_eq!(cfg.review_agent.as_deref(), Some("codex"));
        // A file-set serve_host propagates through load_config (not just the default path).
        assert_eq!(cfg.serve_host, "0.0.0.0");
        assert!(cfg.agents.contains_key("claude"));
        assert!(cfg.agents.contains_key("codex"));
        assert_eq!(
            cfg.reviewer_launch(Some("claude")).unwrap(),
            "claude --permission-mode plan"
        );
        assert_eq!(
            cfg.reviewer_launch(Some("codex")).unwrap(),
            "codex --sandbox read-only"
        );
    }

    #[test]
    fn file_omitting_agents_keeps_builtin_claude() {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("drovr");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("config.toml"), "default_agent = \"claude\"\n").unwrap();
        set_config_home(tmp.path());

        let cfg = load_config().unwrap();
        assert_eq!(
            cfg.reviewer_launch(Some("claude")).unwrap(),
            "claude --permission-mode plan"
        );
    }

    #[test]
    fn worktree_isolation_defaults_on() {
        // On by default in both paths: no config file (Config::default) and a
        // config file that omits the key (serde default).
        assert!(Config::default().worktree, "Config::default should isolate");

        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("drovr");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("config.toml"), "default_agent = \"claude\"\n").unwrap();
        set_config_home(tmp.path());
        assert!(
            load_config().unwrap().worktree,
            "a config omitting `worktree` should still isolate"
        );

        // Explicit opt-out is honored.
        std::fs::write(dir.join("config.toml"), "worktree = false\n").unwrap();
        assert!(
            !load_config().unwrap().worktree,
            "worktree = false must win"
        );
    }

    #[test]
    fn user_agent_map_keeps_missing_builtins_and_fields() {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("drovr");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("config.toml"),
            "[agents.codex]\ncommand = \"codex\"\nreadonly_flag = \"--sandbox read-only\"\n",
        )
        .unwrap();
        set_config_home(tmp.path());

        let cfg = load_config().unwrap();
        assert!(cfg.reviewer_launch(Some("codex")).is_ok());
        assert!(cfg.reviewer_launch(Some("claude")).is_ok());
        assert!(cfg.reviewer_launch(Some("cursor")).is_ok());
        assert_eq!(
            cfg.launch("codex", "/tmp/project", false, None)
                .unwrap()
                .command(),
            "codex -C '/tmp/project'"
        );
    }

    #[test]
    fn reviewer_launch_errors_for_unknown_and_flagless_agents() {
        let cfg = Config {
            default_agent: "claude".into(),
            review_agent: None,
            angles: default_angles(),
            serve_host: default_serve_host(),
            worktree: false,
            reap_finished_panes: true,
            reflex: ReflexConfig::default(),
            agents: {
                let mut m = BTreeMap::new();
                m.insert(
                    "noflag".to_string(),
                    AgentSpec {
                        command: "noflag".into(),
                        readonly_flag: None,
                        readonly_env_unset: Vec::new(),
                        readonly_displace: Vec::new(),
                        workspace: None,
                        system_prompt_flag: None,
                        model_flag: None,
                        review_model: None,
                        resume: None,
                        mcp: None,
                    },
                );
                m
            },
        };
        assert!(cfg.reviewer_launch(Some("does-not-exist")).is_err());
        assert!(cfg.reviewer_launch(Some("noflag")).is_err());
    }

    #[test]
    fn reviewer_launch_none_resolves_overridden_default_agent() {
        let cfg = Config {
            default_agent: "codex".into(),
            review_agent: None,
            angles: default_angles(),
            serve_host: default_serve_host(),
            worktree: false,
            reap_finished_panes: true,
            reflex: ReflexConfig::default(),
            agents: {
                let mut m = BTreeMap::new();
                m.insert(
                    "codex".to_string(),
                    AgentSpec {
                        command: "codex".into(),
                        readonly_env_unset: Vec::new(),
                        readonly_displace: Vec::new(),
                        readonly_flag: Some("--sandbox read-only".into()),
                        workspace: None,
                        system_prompt_flag: None,
                        model_flag: None,
                        review_model: None,
                        resume: None,
                        mcp: None,
                    },
                );
                m
            },
        };
        // None must resolve self.default_agent ("codex"), not the built-in "claude".
        assert_eq!(
            cfg.reviewer_launch(None).unwrap(),
            "codex --sandbox read-only"
        );
    }

    #[test]
    fn reviewer_launch_none_errors_when_default_agent_lacks_flag() {
        let cfg = Config {
            default_agent: "noflag".into(),
            review_agent: None,
            angles: default_angles(),
            serve_host: default_serve_host(),
            worktree: false,
            reap_finished_panes: true,
            reflex: ReflexConfig::default(),
            agents: {
                let mut m = BTreeMap::new();
                m.insert(
                    "noflag".to_string(),
                    AgentSpec {
                        command: "noflag".into(),
                        readonly_flag: None,
                        readonly_env_unset: Vec::new(),
                        readonly_displace: Vec::new(),
                        workspace: None,
                        system_prompt_flag: None,
                        model_flag: None,
                        review_model: None,
                        resume: None,
                        mcp: None,
                    },
                );
                m
            },
        };
        assert!(cfg.reviewer_launch(None).is_err());
    }

    #[test]
    fn reflex_config_parses_full_table() {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("drovr");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("config.toml"),
            r#"
[reflex]
enabled = false
preamble = "Custom framing text."

[reflex.sections]
always-review = false
escalation = true
"#,
        )
        .unwrap();
        set_config_home(tmp.path());

        let cfg = load_config().unwrap();
        assert!(!cfg.reflex.enabled);
        assert_eq!(cfg.reflex.preamble.as_deref(), Some("Custom framing text."));
        assert_eq!(cfg.reflex.sections.get("always-review"), Some(&false));
        assert_eq!(cfg.reflex.sections.get("escalation"), Some(&true));
        // An unlisted section has no override entry.
        assert_eq!(cfg.reflex.sections.get("single-writer"), None);
    }

    #[test]
    fn reflex_table_present_but_enabled_omitted_defaults_true() {
        // The bare-`#[serde(default)]`-on-bool trap: an absent `enabled` under a
        // present `[reflex]` table must fall back to `true`, not `bool::default()`.
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("drovr");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("config.toml"),
            "[reflex]\npreamble = \"only a preamble here\"\n",
        )
        .unwrap();
        set_config_home(tmp.path());

        let cfg = load_config().unwrap();
        assert!(cfg.reflex.enabled);
        assert_eq!(cfg.reflex.preamble.as_deref(), Some("only a preamble here"));
    }

    #[test]
    fn malformed_toml_errors() {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("drovr");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("config.toml"), "this is = = not valid toml [[[").unwrap();
        set_config_home(tmp.path());

        assert!(load_config().is_err());
    }

    #[test]
    fn empty_serve_host_errors() {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("drovr");
        std::fs::create_dir_all(&dir).unwrap();
        // Explicit empty string is a mistake, not a request for the default.
        std::fs::write(dir.join("config.toml"), "serve_host = \"\"\n").unwrap();
        set_config_home(tmp.path());

        assert!(load_config().is_err());
    }

    #[test]
    fn review_agent_prefers_available_cursor_command() {
        let dir = tempfile::tempdir().unwrap();
        let agent = dir.path().join("agent");
        std::fs::write(&agent, "#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            let mut permissions = std::fs::metadata(&agent).unwrap().permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(&agent, permissions).unwrap();
        }

        let mut cfg = Config::default();
        cfg.agents.get_mut("cursor").unwrap().command = agent.to_string_lossy().into_owned();
        assert_eq!(
            cfg.review_agent_for(Some("claude"), true).unwrap(),
            "cursor",
        );
    }

    #[test]
    fn review_agent_falls_back_and_honors_override() {
        let mut cfg = Config::default();
        cfg.agents.get_mut("cursor").unwrap().command = "/definitely/not/a/real/drovr-agent".into();
        assert_eq!(cfg.review_agent_for(Some("codex"), true).unwrap(), "codex",);

        cfg.review_agent = Some("claude".into());
        assert_eq!(cfg.review_agent_for(Some("codex"), true).unwrap(), "claude",);
    }

    #[test]
    fn review_agent_requires_cursor_integration_for_auto_preference() {
        let dir = tempfile::tempdir().unwrap();
        let agent = dir.path().join("agent");
        std::fs::write(&agent, "#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            let mut permissions = std::fs::metadata(&agent).unwrap().permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(&agent, permissions).unwrap();
        }

        let mut cfg = Config::default();
        cfg.agents.get_mut("cursor").unwrap().command = agent.to_string_lossy().into_owned();
        assert_eq!(
            cfg.review_agent_for(Some("claude"), false).unwrap(),
            "claude",
        );
    }
}
