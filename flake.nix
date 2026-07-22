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

          # The crate lives in cli/; all baked-in assets (web/, assets/) are
          # under cli/ too, so the crate root is a self-contained src.
          src = ./cli;

          cargoLock.lockFile = ./cli/Cargo.lock;

          # Install the Claude Code plugin assets (skills + plugin manifest)
          # alongside the binary. These live at the flake root, not under cli/,
          # so reference them via the flake source paths (Nix copies them into
          # the store, and we copy them out into $out/share/drovr/).
          postInstall = ''
            mkdir -p $out/share/drovr
            cp -r ${./skills} $out/share/drovr/skills
            cp -r ${./.claude-plugin} $out/share/drovr/.claude-plugin
          '';

          # doCheck stays on (default). The suite is hermetic: unit tests use an
          # in-process fake, and the e2e test returns early ("skipped-but-passing")
          # when herdr/claude are absent from PATH — which they are inside the
          # Nix build sandbox — so it never reaches the loopback bind.

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
