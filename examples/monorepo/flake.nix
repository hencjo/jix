{
  inputs.jix.url = "path:../..";
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
          deps = jix.workspaceDependencies {
            inherit pkgs;
            lockFile = ./jix.lock;
            specs.parser = (import ./build-support/parser/default.nix).spec;
          };
          java = jix.lib { inherit pkgs; };
          build = import ./build.nix { inherit java deps; };
        in
        jix.project {
          inherit pkgs deps;
          targets = build;
          defaultTarget = "application";
          apps = {
            server = {
              type = "app";
              program = "${build.application}/bin/server";
            };
            plain = {
              type = "app";
              program = "${build.plainApp}/bin/plain";
            };
          };
          defaultApp = "server";
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
