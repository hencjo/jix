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
  thin = java.library {
    name = "application";
    sources = ./src/main/java;
    compile.classpath = [ deps.classpaths.main ];
    inherit runtime;
    inherit jdk;
    release = 21;
    resources = [ { from = ./src/main/resources; } ];
    manifest."Main-Class" = "example.Main";
  };
  jar = java.fatJar {
    name = "application";
    dependencies = [ thin ];
    mainClass = "example.Main";
  };
  app =
    (pkgs.writeShellApplication {
      name = "application";
      text = "exec ${jdk}/bin/java -jar ${jar} \"$@\"";
    })
    // {
      inherit (jar) jixApplication;
    };
in
{
  inherit jar app;
  inherit (thin) classes;
}
