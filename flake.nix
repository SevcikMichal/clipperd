{
  description = "clipperd — seamless iPhone ↔ Linux clipboard sync over your LAN";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
      pkgsFor = system: nixpkgs.legacyPackages.${system};
    in
    {
      packages = forAllSystems (system:
        let pkgs = pkgsFor system; in {
          clipperd = pkgs.callPackage ./nix/package.nix { };
          default = self.packages.${system}.clipperd;
        });

      apps = forAllSystems (system: {
        default = {
          type = "app";
          program = "${self.packages.${system}.clipperd}/bin/clipperd";
        };
      });

      overlays.default = final: prev: {
        clipperd = final.callPackage ./nix/package.nix { };
      };

      devShells = forAllSystems (system:
        let pkgs = pkgsFor system; in {
          default = pkgs.mkShell {
            packages = with pkgs; [ cargo rustc rust-analyzer clippy rustfmt libnotify ];
          };
        });

      homeManagerModules.default = import ./nix/hm-module.nix { inherit self; };
    };
}
