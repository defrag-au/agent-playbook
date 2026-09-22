{
  description = "Resolve and render agent rules for a project + target, and the at-* inspection toolkit";

  # Two inputs, both public. `fenix` is the toolchain every other repository in the org
  # builds with, and pinning the same one here is worth the input it costs: `nix build`,
  # the devshell below, and the sibling repos all agree, rather than this being a second
  # rustc that drifts on the next bump.
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    { nixpkgs, fenix, ... }:
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

      # Assembled the way defrag-nix assembles it, from the same fenix channel, so a bump in both
      # places leaves every repository on one rustc rather than this one drifting. The pin is
      # matched, not inherited, and could not be inherited: defrag-nix takes this repository as an
      # input (for `agent-tools`), so an input back would be a cycle. No wasm targets: nothing here
      # builds for wasm.
      rustToolchainFor =
        system:
        let
          fenixPkgs = fenix.packages.${system};
        in
        fenixPkgs.combine [
          fenixPkgs.stable.cargo
          fenixPkgs.stable.clippy
          fenixPkgs.stable.rustc
          fenixPkgs.stable.rustfmt
        ];

      # `makeRustPlatform` wants cargo and rustc separately; a combined toolchain carries
      # both, so it is passed twice.
      rustPlatformFor =
        pkgs:
        let
          toolchain = rustToolchainFor pkgs.stdenv.hostPlatform.system;
        in
        pkgs.makeRustPlatform {
          cargo = toolchain;
          rustc = toolchain;
        };

      # The composer: reads rules/ for a project and target and writes a repo's block.
      #
      # It carries its own data tree in `share/playbook`, so an installed binary is
      # self-locating: `rules/`, `models/`, `projects/`, `memory/` and `templates/` are read
      # relative to the executable when there is no checkout to walk up from. Without this the
      # only root an installed composer could name is the build directory it was compiled in,
      # which is gone by the time anyone runs it — see `find_root` in src/load.rs.
      composerFor =
        rustPlatform:
        rustPlatform.buildRustPackage {
          pname = "playbook";
          version = (lib.importTOML ./Cargo.toml).package.version;
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;
          cargoBuildFlags = [ "-p" "agent-playbook" ];
          postInstall = ''
            mkdir -p $out/share/playbook
            cp -R rules models projects memory templates $out/share/playbook/
          '';
        };

      # The at-* toolkit. One derivation for every binary: they are tiers of the same toolkit — a
      # catalogue, a working-tree reader, a history reader — and are always installed together, and
      # a fourth tool is a line in cargoBuildFlags and a line in installPhase.
      #
      # Built with the same toolchain the devshell below provides, from the same fenix pin
      # the sibling repositories use.
      #
      # Not a `cargo run` shim over a checkout: an agent working in another repository may
      # write nothing outside its own tree, so a first-invocation compile elsewhere fails
      # in a way that reads like a permissions problem. A store path also cannot be
      # re-pointed by editing this repository — which matters, because the whole point of
      # these binaries is to be allowlisted once and trusted thereafter.
      toolkitFor =
        rustPlatform:
        rustPlatform.buildRustPackage {
          pname = "agent-tools";
          # From the crate that names the toolkit, so the two cannot drift.
          version = (lib.importTOML ./tools/at-peek/Cargo.toml).package.version;
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;
          cargoBuildFlags = [
            "-p"
            "at-peek"
            "-p"
            "at-recall"
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
            for tool in at-peek at-recall at-describe; do
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
          rustPlatform = rustPlatformFor (pkgsFor system);
          playbook = composerFor rustPlatform;
          agent-tools = toolkitFor rustPlatform;
        in
        {
          inherit playbook agent-tools;
          # What this repository is *for*; the toolkit is what it also ships.
          default = playbook;
        }
      );

      # `nix develop` here, so this repository can follow its own
      # `rules/rust/devshell-first` — which it could not before this flake existed. The
      # shell's rustc and cargo are the ones the packages are built with, because both
      # come from this one combined toolchain.
      devShells = forAllSystems (
        system:
        let
          pkgs = pkgsFor system;
        in
        {
          default = pkgs.mkShell {
            packages = [ (rustToolchainFor system) ];
          };
        }
      );

      # `nix flake check` runs the suite rather than only evaluating the outputs.
      checks = forAllSystems (
        system:
        let
          pkgs = pkgsFor system;
          rustPlatform = rustPlatformFor pkgs;
          playbook = composerFor rustPlatform;
        in
        {
          # Through `buildRustPackage` rather than a bare `runCommand` with the toolchain on PATH:
          # the workspace has a real dependency now (`at-peek` uses `regex`), and a bare build has
          # no registry and no vendoring, so it cannot resolve it — which is what this entry did
          # from the day the toolkit landed until now. The vendoring here comes from the same
          # `cargoLock` the packages use.
          #
          # `git` is on the PATH for the same reason it is in a devshell: the contract tests drive
          # the real binary, and the binary runs `git` against a fixture repository.
          tests = rustPlatform.buildRustPackage {
            pname = "agent-playbook-tests";
            version = (lib.importTOML ./Cargo.toml).package.version;
            src = ./.;
            cargoLock.lockFile = ./Cargo.lock;
            nativeBuildInputs = [ pkgs.git ];
            buildPhase = "cargo build --workspace --offline";
            doCheck = true;
            checkPhase = "cargo test --workspace --offline";
            installPhase = "touch $out";
          };

          # The composer finds its own rule tree from a directory that is not a checkout. That is
          # the whole reason the tree ships in `share/playbook`: a packaged binary whose only
          # fallback was the directory it was compiled in would resolve to nothing, and the
          # failure would first be seen by whoever ran `playbook check` from a devshell.
          self-locating = pkgs.runCommand "playbook-self-locating" { } ''
            ${playbook}/bin/playbook root | grep -q "${playbook}/share/playbook"
            touch $out
          '';
        }
      );
    };
}
