{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    # The Rust toolchain, with the standard library for the targets Bacre is built for
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, fenix }:
    let
      # Where the dev shell runs: this laptop, and the Linux machines of a build pipeline
      systems = [ "aarch64-darwin" "x86_64-linux" "aarch64-linux" ];
      # What `just build` produces: static Linux binaries (keep in step with the justfile)
      targets = [ "x86_64-unknown-linux-musl" "aarch64-unknown-linux-musl" ];
      forEachSystem = f: nixpkgs.lib.genAttrs systems (system: f system);
    in {
      devShells = forEachSystem (system:
        let
          pkgs = import nixpkgs { inherit system; };
          rust = fenix.packages.${system};
          toolchain = rust.combine ([
            rust.stable.cargo
            rust.stable.rustc           # includes rust-lld, which links the Linux binaries
            rust.stable.rustfmt
            rust.stable.clippy
            rust.stable.rust-src        # for rust-analyzer
            rust.stable.rust-analyzer
          ] ++ map (target: rust.targets.${target}.stable.rust-std) targets);
        in {
          default = pkgs.mkShell {
            packages = [
              toolchain            # Rust
              pkgs.cargo-watch     # Rust: restarts the server when a source file changes
              pkgs.just            # Task runner (see the justfile)
              pkgs.bun             # Bun (for the website)
            ];
          };
        });
    };
}
