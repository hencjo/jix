{
  spec = {
    classpaths.main.dependencies = [ ];
  };
  lockFile = ./jix.lock;
  build =
    {
      java,
      src,
      deps,
      ...
    }:
    java.library {
      name = "fixture.parser";
      jarName = "parser.jar"; # Stable automatic module name: parser.
      sources = src + "/src/main/java";
      dependencies = [ deps.classpaths.main ];
    };
}
