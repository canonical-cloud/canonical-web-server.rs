{
  description = "canonical-web-server.rs development environment";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs = { nixpkgs, ... }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
    in
    {
      devShells = forAllSystems (system:
        let
          pkgs = import nixpkgs { inherit system; };
        in
        {
          default = pkgs.mkShell {
            packages = with pkgs; [
              # encrypted env files — env/enc/*.env.enc, see env/README.md
              sops
              age
              python3
              rustc
              cargo
              rustfmt
              clippy
              rust-analyzer

              git
              direnv
              just
              bacon

              # Handy for building/serving the sibling Astro frontend locally.
              nodejs

              pkg-config
              openssl
            ];

            shellHook = ''
              echo "canonical-web-server dev shell (${system})"
            '';
          };
        });
    };
}
