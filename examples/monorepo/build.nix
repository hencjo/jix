{ java, deps }:
let
  parser = java.importLibrary {
    descriptor = ./build-support/parser/default.nix;
    src = ./vendor/parser;
    deps = deps.components.parser; # Omit to use the adapter's independent lock.
  };
  core = java.library {
    name = "com.example.core";
    sources = ./libraries/core/src;
    resources = [ { from = ./libraries/core/resources; } ];
    moduleDependencies = [ parser ];
  };
  server = java.library {
    name = "com.example.server";
    sources = ./applications/server/src;
    moduleDependencies = [ core ];
  };
  plugin = java.library {
    name = "fixture.plugin";
    sources = ./libraries/plugin/src;
  };
  plain = java.library {
    name = "fixture.plain";
    sources = ./applications/plain/src;
    dependencies = [ parser ];
    runtimeDependencies = [ plugin ];
  };
  mixed = java.library {
    name = "fixture.mixed";
    sources = ./applications/mixed/src;
    moduleDependencies = [ core ];
  };
  mixedApp = java.application {
    name = "mixed";
    dependencies = [ mixed ];
    mainClass = "mixed.Main";
  };
  unrelated = java.library {
    name = "fixture.unrelated";
    sources = ./libraries/unrelated/src;
  };
  application = java.application {
    name = "server";
    moduleDependencies = [ server ];
    module = "com.example.server/server.Main";
  };
  plainApp = java.application {
    name = "plain";
    dependencies = [ plain ];
    mainClass = "plain.Main";
  };
in
{
  inherit
    parser
    core
    server
    unrelated
    plugin
    plain
    application
    plainApp
    mixed
    mixedApp
    ;
}
