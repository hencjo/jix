# Optional Java build DSL: dependency consumers need not use this file.
{
  pkgs,
  java,
  deps,
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
    jdk = pkgs.jdk21;
    release = 21;
    resources = [ { from = ./src/main/resources; } ];
    manifest."Main-Class" = "example.Main";
  };
  jar = java.fatJar {
    name = "application";
    dependencies = [ thin ];
    mainClass = "example.Main";
    # Explicit relocation, not Gradle Shadow compatibility.
    relocations = [
      {
        from = "example";
        to = "internal.example";
      }
    ];
    relocateStringConstants = true;
  };
  app =
    (pkgs.writeShellApplication {
      name = "application";
      text = "exec ${pkgs.jdk21}/bin/java -jar ${jar} \"$@\"";
    })
    // {
      inherit (jar) jixApplication;
    };
in
{
  inherit jar app;
  inherit (thin) classes;
}
