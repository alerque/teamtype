# SPDX-FileCopyrightText: 2026 Caleb Maclennan <caleb@alerque.com>
# SPDX-FileCopyrightText: 2026 blinry <mail@blinry.org>
#
# SPDX-License-Identifier: AGPL-3.0-or-later
{
  description = "Enables real-time co-editing of local text files.";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs =
    { self, ... }@inputs:
    inputs.flake-parts.lib.mkFlake { inherit inputs; } {
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];
      perSystem =
        {
          config,
          self',
          pkgs,
          lib,
          system,
          ...
        }:
        let
          runtimeDeps = with pkgs; [
            libgit2
          ];
          buildDeps = with pkgs; [
            git
            pkg-config
          ];
          devDeps = with pkgs; [
            cargo-deny
            git
            just
            luaPackages.luacheck
            prettier
            reuse
            rustup
            stylua
            typos
          ];

          workspaceToml = builtins.fromTOML (builtins.readFile ./Cargo.toml);
          cargoToml = builtins.fromTOML (builtins.readFile ./crates/teamtype/Cargo.toml);

          resolveManifestValue =
            key:
            if builtins.isAttrs cargoToml.package.${key} && cargoToml.package.${key} ? workspace then
              workspaceToml.workspace.package.${key}
            else
              cargoToml.package.${key};

          msrv = resolveManifestValue "rust-version";

          rustPlatform = pkgs.makeRustPlatform {
            cargo = pkgs.rust-bin.stable.latest.minimal;
            rustc = pkgs.rust-bin.stable.latest.minimal;
          };

          rustPackage =
            features:
            rustPlatform.buildRustPackage {
              name = resolveManifestValue "name";
              version = resolveManifestValue "version";
              src = lib.cleanSourceWith {
                src = ./.;
                filter = path: type:
                  baseNameOf path == ".git" || lib.cleanSourceFilter path type;
              };
              cargoLock.lockFile = ./Cargo.lock;
              buildFeatures = features;
              buildInputs = runtimeDeps;
              nativeBuildInputs = buildDeps;
              env = {
                LIBGIT2_NO_VENDOR = 1;
              };
              # Populate vergen env vars ahead of time if Nix has access to git history.
              preConfigure = ''
                if git rev-parse --git-dir > /dev/null 2>&1; then
                  export VERGEN_GIT_SHA="$(git rev-parse HEAD)"
                  export VERGEN_GIT_DESCRIBE="$(git describe --long --tags --match "v${resolveManifestValue "version"}" --abbrev=9)"
                  export VERGEN_GIT_COMMIT_DATE="$(git show -s --format=%cs HEAD)"
                  export VERGEN_GIT_DIRTY="${lib.boolToString (self ? dirtyRev)}"
                  env
                else
                  ls -al
                fi
                exit 1
              '';
              doCheck = false;
            };

          mkDevShell = pkgs.mkShell {
            buildInputs = runtimeDeps;
            nativeBuildInputs = buildDeps ++ devDeps;
          };
        in
        {
          _module.args.pkgs = import inputs.nixpkgs {
            inherit system;
            overlays = [ (import inputs.rust-overlay) ];
          };

          packages.teamtype = rustPackage [ ];
          packages.default = self'.packages.teamtype;
          devShells.default = mkDevShell;
          formatter = pkgs.nixfmt;
        };
    };
}
