{ pkgs, jix }:
let
  spec = jix.dependencySpec (import ./maven.nix);
  dependencies = jix.dependencies {
    inherit spec;
    lockFile = ./jix.lock;
  };
  classpaths = dependencies.classpaths;
  classes = jix.compileJava {
    name = "hello-classes";
    sources = ./src/main/java;
    classpaths = [ classpaths.main ];
    jdk = pkgs.jdk21;
    release = 21;
  };
  app = jix.jar {
    name = "hello";
    inherit classes;
    resources = [
      {
        from = ./src/main/resources;
        into = "";
      }
    ];
    manifest."Main-Class" = "example.Main";
  };
  launcher = pkgs.writeShellApplication {
    name = "jix-hello";
    runtimeInputs = [
      pkgs.jdk21
      pkgs.coreutils
    ];
    text = ''exec java -cp "${app}:$(cat ${classpaths.runtime.classpathFile})" example.Main "$@"'';
  };
in
{
  inherit
    spec
    dependencies
    classpaths
    classes
    app
    launcher
    ;
}
