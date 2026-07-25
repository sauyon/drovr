{
  description = "drovr — CLI for driving multi-agent code review/handoff pipelines";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
      in
      {
        packages.default = pkgs.rustPlatform.buildRustPackage {
          pname = "drovr";
          version = "0.1.0";

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

        devShells.default = pkgs.mkShell {
          packages = with pkgs; [
            cargo
            rustc
            rust-analyzer
          ];
        };
      });
}
