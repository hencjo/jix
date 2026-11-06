{
  pkgs,
  java,
  deps,
  jdk ? pkgs.jdk21,
}:
let
  core = java.library {
    name = "core";
    sources = ./core/src/main/java;
    inherit jdk;
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
    inherit jdk;
    release = 21;
  };
  app = java.application {
    name = "application";
    runtime = runtime // {
      classpath = [ jar ] ++ runtime.classpath;
    };
    mainClass = "example.Main";
    inherit jdk;
  };
in
{
  inherit core jar app;
}
