{
  description = "Resolve and render agent rules for a project + target, and the at-* inspection toolkit";

  # One input, deliberately. This repository is meant to be usable by someone who has
  # never heard of the org it came from, so a stranger can `nix build` it with nothing
  # else to fetch — which rules out borrowing a toolchain from a sibling repo.
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

  outputs =
    { nixpkgs, ... }:
    let
      lib = nixpkgs.lib;
      systems = [
        "aarch64-darwin"
        "x86_64-darwin"
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems = lib.genAttrs systems;
      pkgsFor = system: import nixpkgs { inherit system; };

      # The composer: reads rules/ for a project and target and writes a repo's block.
      composerFor =
        pkgs:
        pkgs.rustPlatform.buildRustPackage {
          pname = "playbook";
          version = (lib.importTOML ./Cargo.toml).package.version;
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;
          cargoBuildFlags = [ "-p" "agent-playbook" ];
        };

      # The at-* toolkit. One derivation for both binaries: they are two trust tiers of
      # the same toolkit and are always installed together, and a third tool is a line in
      # cargoBuildFlags and a line in installPhase.
      #
      # Built with the same rustPlatform this flake's devshell provides, so the package
      # and `nix develop -c cargo test` agree about the toolchain instead of being two
      # definitions that drift apart.
      #
      # Not a `cargo run` shim over a checkout: an agent working in another repository may
      # write nothing outside its own tree, so a first-invocation compile elsewhere fails
      # in a way that reads like a permissions problem. A store path also cannot be
      # re-pointed by editing this repository — which matters, because the whole point of
      # these binaries is to be allowlisted once and trusted thereafter.
      toolkitFor =
        pkgs:
        pkgs.rustPlatform.buildRustPackage {
          pname = "agent-tools";
          # From the crate that names the toolkit, so the two cannot drift.
          version = (lib.importTOML ./tools/at-peek/Cargo.toml).package.version;
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;
          cargoBuildFlags = [
            "-p"
            "at-peek"
            "-p"
            "at-describe"
          ];
          # `cargo install` takes one package per invocation and the workspace root is the
          # composer, so the default install phase would install `playbook` and neither
          # tool. Copy from the build output instead, searching for it: cargoBuildHook
          # passes `--target <triple>`, so the artefacts land in `target/<triple>/release/`
          # and not `target/release/`, and a miss fails loudly on `install`.
          installPhase = ''
            runHook preInstall
            mkdir -p $out/bin
            for tool in at-peek at-describe; do
              install -m755 \
                "$(find target -maxdepth 3 -type f -name "$tool" -perm -u+x | head -n1)" \
                "$out/bin/$tool"
            done
            runHook postInstall
          '';
        };
    in
    {
      packages = forAllSystems (
        system:
        let
          pkgs = pkgsFor system;
          playbook = composerFor pkgs;
          agent-tools = toolkitFor pkgs;
        in
        {
          inherit playbook agent-tools;
          # What this repository is *for*; the toolkit is what it also ships.
          default = playbook;
        }
      );

      # `nix develop` here, so this repository can follow its own
      # `rules/rust/devshell-first` — which it could not before this flake existed. The
      # four components come from the same rustPlatform the packages are built with.
      devShells = forAllSystems (
        system:
        let
          pkgs = pkgsFor system;
        in
        {
          default = pkgs.mkShell {
            packages = with pkgs; [
              cargo
              clippy
              rustc
              rustfmt
            ];
          };
        }
      );

      # `nix flake check` runs the suite rather than only evaluating the outputs. The
      # workspace has no dependencies, so this needs no registry and no vendoring: copy the
      # source out of the store (read-only) and run the tests from the copy.
      checks = forAllSystems (
        system:
        let
          pkgs = pkgsFor system;
        in
        {
          tests = pkgs.runCommand "agent-playbook-tests" {
            nativeBuildInputs = [
              pkgs.cargo
              pkgs.rustc
            ];
          } ''
            cp -r ${./.} src
            chmod -R u+w src
            cd src
            export CARGO_HOME="$TMPDIR/cargo"
            cargo test --workspace --offline --locked
            touch $out
          '';
        }
      );
    };
}
