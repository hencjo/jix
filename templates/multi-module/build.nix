{
  pkgs,
  java,
  deps,
}:
let
  core = java.library {
    name = "core";
    sources = ./core/src/main/java;
    jdk = pkgs.jdk21;
    release = 21;
  };
  runtime = {
    classpath = [
      core
      deps.classpaths.runtime
    ];
  };
  jar = java.library {
    name = "application";
    sources = ./app/src/main/java;
    compile.classpath = [
      core
      deps.classpaths.main
    ];
    inherit runtime;
    jdk = pkgs.jdk21;
    release = 21;
  };
  app = java.application {
    name = "application";
    runtime = runtime // {
      classpath = [ jar ] ++ runtime.classpath;
    };
    mainClass = "example.Main";
    jdk = pkgs.jdk21;
  };
in
{
  inherit core jar app;
}
