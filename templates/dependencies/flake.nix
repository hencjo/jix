{
  inputs.jix.url = "@JIX_SOURCE@";
  inputs.nixpkgs.follows = "jix/nixpkgs";
  outputs =
    { jix, nixpkgs, ... }:
    let
      each = nixpkgs.lib.genAttrs [
        "x86_64-linux"
        "aarch64-linux"
      ];
      projects = each (
        system:
        let
          pkgs = import nixpkgs { inherit system; };
          deps = jix.dependencies {
            inherit pkgs;
            spec = import ./maven.nix;
            lockFile = ./jix.lock;
          };
        in
        jix.project {
          inherit pkgs deps;
        }
      );
    in
    {
      jix = each (system: {
        dependencies = projects.${system}.descriptor;
      });
      packages = each (system: projects.${system}.packages);
      apps = each (system: projects.${system}.apps);
    };
}
