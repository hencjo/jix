{
  inputs.jix.url = "path:../..";
  inputs.nixpkgs.follows = "jix/nixpkgs";
  outputs =
    { jix, nixpkgs, ... }:
    let
      each = nixpkgs.lib.genAttrs [
        "x86_64-linux"
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
          build = import ./build.nix {
            inherit pkgs deps;
            java = jix.lib { inherit pkgs; };
          };
        in
        jix.project {
          inherit pkgs deps;
          targets = {
            application = build.app;
            jar = build.jar;
          };
          defaultTarget = "application";
          apps.application = {
            type = "app";
            program = "${build.app}/bin/application";
          };
          defaultApp = "application";
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
