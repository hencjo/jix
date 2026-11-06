# Optional Java build DSL: dependency consumers need not use this file.
{
  pkgs,
  java,
  deps,
  jdk ? pkgs.jdk21,
}:
let
  runtime = {
    classpath = [ deps.classpaths.runtime ];
  };
  jar = java.library {
    name = "application";
    sources = ./src/main/java;
    compile.classpath = [ deps.classpaths.main ];
    inherit runtime;
    inherit jdk;
    release = 21;
    resources = [ { from = ./src/main/resources; } ];
    manifest."Main-Class" = "example.Main";
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
  inherit jar app;
  inherit (jar) classes;
}
