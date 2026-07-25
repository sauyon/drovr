{
  description = "drovr — CLI for driving multi-agent code review/handoff pipelines";

  # Inputs are pinned to immutable commit revs rather than moving branch refs
  # (`nixpkgs-unstable`, `master`) so a fresh `nix build` on a machine with a
  # cold evaluation cache resolves to exactly the tree recorded in flake.lock —
  # an upstream force-push or a re-pointed branch cannot silently change what we
  # build. To update: bump the `rev` below to the desired commit, then run
  # `nix flake update nixpkgs` (or `nix flake update flake-utils`).
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/7525d999cd850b9a488817abc89c75dc733acf17";
    flake-utils.url = "github:numtide/flake-utils/11707dc2f618dd54ca8739b309ec4fc024de578b";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
      in
      {
        packages.default = pkgs.rustPlatform.buildRustPackage {
          pname = "drovr";
          version = "0.2.0";

          # The crate lives in cli/, but two integration tests reach OUT to
          # repo-root siblings during checkPhase: reflex_hook execs
          # hooks/session-start (which itself reads skills/using-drovr), and
          # skills_valid scans skills/. Both resolve those via
          # CARGO_MANIFEST_DIR/.. — so the src must be the whole repo, with cargo
          # building/testing in the cli/ subdir, or ../hooks and ../skills are
          # absent in the sandbox and those tests fail (127 / read errors).
          src = ./.;
          buildAndTestSubdir = "cli";
          # Cargo.lock/Cargo.toml live in cli/, not the source root, so point the
          # cargo setup hooks (vendoring + lockfile consistency check) there.
          cargoRoot = "cli";

          cargoLock.lockFile = ./cli/Cargo.lock;

          # The code_review / phase tests build a throwaway git repo (via
          # `Command::new("git")`) to exercise head-SHA resolution. git isn't on
          # the Nix build sandbox PATH by default, so those `git` calls returned
          # NotFound and panicked while holding the shared ENV_LOCK — poisoning
          # it and cascading ~70 downstream tests into PoisonError. Provide git
          # to the check phase so the suite runs as it does locally.
          nativeCheckInputs = [ pkgs.git ];

          # Install the Claude Code plugin assets (skills + plugin manifest)
          # alongside the binary. These live at the flake root, not under cli/,
          # so reference them via the flake source paths (Nix copies them into
          # the store, and we copy them out into $out/share/drovr/).
          postInstall = ''
            mkdir -p $out/share/drovr
            cp -r ${./skills} $out/share/drovr/skills
            cp -r ${./.claude-plugin} $out/share/drovr/.claude-plugin
          '';

          # doCheck stays on (default). The suite is hermetic given git (see
          # nativeCheckInputs above): unit tests use an in-process fake, and the
          # e2e test returns early ("skipped-but-passing") when herdr/claude are
          # absent from PATH — which they are inside the Nix build sandbox — so
          # it never reaches the loopback bind.

          meta = with pkgs.lib; {
            description = "CLI for driving multi-agent code review/handoff pipelines";
            license = licenses.mit;
            mainProgram = "drovr";
            platforms = platforms.unix;
          };
        };

        # Supply-chain gate: enforce cli/deny.toml. Runs the deterministic,
        # network-free subset (bans/licenses/sources) inside the sandbox so a
        # dependency from an unexpected registry/git source, an unapproved
        # license, or a wildcard version fails `nix flake check` reproducibly.
        # The `advisories` check is intentionally NOT run here — it wants the
        # live RustSec DB, so it belongs in CI/dev (`cargo deny check advisories`
        # from the devShell), not pinned to a stale sandbox snapshot.
        checks.cargo-deny =
          let
            vendorDir = pkgs.rustPlatform.importCargoLock {
              lockFile = ./cli/Cargo.lock;
            };
          in
          pkgs.runCommand "drovr-cargo-deny"
            {
              nativeBuildInputs = [ pkgs.cargo-deny pkgs.cargo ];
            }
            ''
              cp -r ${./cli} src && chmod -R +w src
              export CARGO_HOME=$PWD/cargo-home
              mkdir -p $CARGO_HOME
              cat > $CARGO_HOME/config.toml <<EOF
              [source.crates-io]
              replace-with = "vendored-sources"
              [source.vendored-sources]
              directory = "${vendorDir}"
              EOF
              # Point at deny.toml explicitly: relying on cargo-deny's implicit
              # CWD discovery risks a future version silently falling back to its
              # permissive default config and passing without enforcing anything.
              # --config makes a missing config a hard error instead.
              cd src
              cargo-deny --offline --manifest-path Cargo.toml \
                --config deny.toml \
                check bans licenses sources
              touch $out
            '';

        devShells.default = pkgs.mkShell {
          packages = with pkgs; [
            cargo
            rustc
            rust-analyzer
            # `cargo deny check` — supply-chain audit (see cli/deny.toml).
            cargo-deny
          ];
        };
      });
}
