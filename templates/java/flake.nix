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
      jdks = each (system: (import nixpkgs { inherit system; }).jdk21);
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
            jdk = jdks.${system};
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
        initJdk = jdks.${system};
        dependencies = projects.${system}.descriptor;
      });
      packages = each (system: projects.${system}.packages);
      apps = each (system: projects.${system}.apps);
    };
}
