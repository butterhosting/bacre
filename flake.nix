{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, fenix }:
    let
      systems = [ "aarch64-darwin" "x86_64-linux" "aarch64-linux" ];
      targets = [ "x86_64-unknown-linux-musl" "aarch64-unknown-linux-musl" ];
      forEachSystem = f: nixpkgs.lib.genAttrs systems (system: f system);
    in {
      devShells = forEachSystem (system:
        let
          pkgs = import nixpkgs { inherit system; };
          rust = fenix.packages.${system};
          toolchain = rust.combine ([
            rust.stable.cargo
            rust.stable.rustc # includes rust-lld, which links the Linux binaries
            rust.stable.rustfmt
            rust.stable.clippy
            rust.stable.rust-src
            rust.stable.rust-analyzer
          ] ++ map (target: rust.targets.${target}.stable.rust-std) targets);
        in {
          default = pkgs.mkShell {
            packages = [
              toolchain
              pkgs.cargo-watch
              pkgs.just
              pkgs.bun
              pkgs.nodejs_24
            ];
          };
        });
    };
}
