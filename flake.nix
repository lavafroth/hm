{
  description = "github:lavafroth/hm CLI utility to render manim animations on save";

  inputs = {
    flake-parts.url = "github:hercules-ci/flake-parts";
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs =
    inputs@{ flake-parts, ... }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "aarch64-darwin"
        "x86_64-darwin"
      ];
      perSystem =
        { pkgs, ... }:
        {
          devShells.default = pkgs.mkShell {
            packages = with pkgs; [ stdenv.cc.cc.lib ];
            LD_LIBRARY_PATH = pkgs.stdenv.cc.cc.lib.LIBRARY_PATH;
          };
          packages.default = pkgs.pkgsStatic.rustPlatform.buildRustPackage {
            pname = "hm";
            version = "1.0.0";
            src = ./.;
            cargoLock.lockFile = ./Cargo.lock;
          };
        };
    };
}
