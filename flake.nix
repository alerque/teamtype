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
            let
              hasGit = self ? rev;
              isDirty = self ? dirtyRev;
              gitRev = if isDirty then self.dirtyRev else if hasGit then self.rev else null;
              # lastModifiedDate is "YYYYMMDDHHMMSS" — reformat to "YYYY-MM-DD"
              commitDate =
                let
                  d = self.lastModifiedDate;
                in
                "${builtins.substring 0 4 d}-${builtins.substring 4 2 d}-${builtins.substring 6 2 d}";
            in
            rustPlatform.buildRustPackage {
              name = resolveManifestValue "name";
              version = resolveManifestValue "version";
              src = self.outPath;
              cargoLock.lockFile = ./Cargo.lock;
              buildFeatures = features;
              buildInputs = runtimeDeps;
              nativeBuildInputs = buildDeps;
              env =
                {
                  LIBGIT2_NO_VENDOR = 1;
                }
                // lib.optionalAttrs hasGit {
                  VERGEN_GIT_SHA = gitRev;
                  VERGEN_GIT_DIRTY = lib.boolToString isDirty;
                  VERGEN_GIT_COMMIT_DATE = commitDate;
                };
              # VERGEN_GIT_DESCRIBE can't be derived from flake metadata alone
              # (it requires tag distance), so we compute it at build time using
              # the original source tree's .git dir when it exists.
              preConfigure = ''
                _gitdir="${toString ./.}/.git"
                if [ -d "$_gitdir" ]; then
                  export VERGEN_GIT_DESCRIBE="$(
                    git --git-dir="$_gitdir" describe \
                      --long --tags \
                      --match "v${resolveManifestValue "version"}" \
                      --abbrev=9 \
                      ${lib.optionalString isDirty "--dirty"}
                  )"
                fi
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
