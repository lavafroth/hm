{
  description = "github:lavafroth/hm CLI utility to render manim animations on save";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { nixpkgs, ... }:
    let
      forAllSystems =
        f:
        nixpkgs.lib.genAttrs nixpkgs.lib.systems.flakeExposed (system: f nixpkgs.legacyPackages.${system});
    in

    {
      devShells = forAllSystems (pkgs: {
        default = pkgs.mkShell {
          packages = with pkgs; [ stdenv.cc.cc.lib ];
          LD_LIBRARY_PATH = pkgs.stdenv.cc.cc.lib.LIBRARY_PATH;
        };
      });
      packages = forAllSystems (pkgs: {
        default = pkgs.pkgsStatic.rustPlatform.buildRustPackage {
          pname = "hm";
          version = "1.0.0";
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;
        };
      });
    };
}
