# Supply-chain posture

drovr ships a compiled binary that embeds third-party assets and spawns external
agent/tooling processes. This document records the controls that keep untrusted
or tampered code from entering that pipeline, and the trust assumptions that
remain.

## Controls

### 1. Embedded vendored assets are pinned by digest

`cli/web/vendor/markdown-it.min.js` is baked into the binary via `include_bytes!`
and served to the reviewer's browser, which renders untrusted diff content. A
tampered copy would run attacker JavaScript locally.

- `cli/web/vendor/PROVENANCE.toml` pins every vendored asset: upstream name,
  version, source URL, byte length, and SHA-256.
- `vendor_integrity_matches_pin` (in `cli/src/review.rs`) hashes the embedded
  bytes with a dependency-free SHA-256 (`cli/src/sha256.rs`, test-only) and
  fails the suite if they drift from the pin.
- `vendor_integrity_provenance_in_sync` fails if `PROVENANCE.toml` and the
  enforced pin disagree, so the audited record can't silently rot.

Updating an asset is a deliberate act: download from the documented URL, verify
against the publisher, replace the file, update the pin **and** `PROVENANCE.toml`,
and re-run `cargo test`.

### 2. Rust dependencies are gated by cargo-deny

`cli/deny.toml` enforces:

- **sources** — only crates.io; unknown registries and git deps are rejected.
- **licenses** — only an explicit permissive allow-list.
- **bans** — wildcard (`*`) version requirements rejected; duplicate versions
  warned.
- **advisories** — RustSec advisories and yanked crates rejected.

`nix flake check` runs the deterministic `bans`/`licenses`/`sources` subset fully
offline (`checks.cargo-deny` in `flake.nix`). The `advisories` check wants the
live RustSec DB, so it runs from the devShell / CI with network:

```sh
nix develop -c cargo deny check advisories   # or: cargo deny check
```

### 3. Nix inputs are pinned to immutable revs

`flake.nix` references `nixpkgs` and `flake-utils` by commit rev, not by moving
branch (`nixpkgs-unstable`). An upstream force-push or re-pointed branch cannot
change what a cold-cache build resolves to. Update by bumping the rev and running
`nix flake update <name>`.

### 4. Spawned commands must not resolve against an untrusted CWD

drovr spawns agent backends (`claude`, `cursor-agent`, `codex`, …) whose command
strings come from `~/.config/drovr/config.toml`. It often runs with its working
directory inside the repository under review.

`config::validate_command` rejects any agent command that is a **relative path**
(`./git`, `bin/agent`). Such a path would resolve against the CWD, letting a
hostile repo drop a lookalike binary. Only bare names (resolved via the trusted
`$PATH`) and absolute paths are accepted.

## Remaining trust assumptions

- **`$PATH` is trusted.** `herdr`, `claude`, `git`, and bare-name agent commands
  are resolved via `$PATH`. drovr does not pin them to absolute paths; a user who
  puts a malicious directory early in `$PATH` is already compromised.
- **The RustSec advisory DB is fetched at audit time**, not pinned. This is
  deliberate — advisory scanning is only useful against the latest DB — but it
  means the `advisories` gate depends on network access wherever it runs.
- **cargo-deny does not verify crate checksums beyond `Cargo.lock`.** Integrity
  of downloaded crates rests on Cargo's own `Cargo.lock` checksum verification
  against crates.io.
