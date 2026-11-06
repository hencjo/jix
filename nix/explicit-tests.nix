{ pkgs, java }:
let
  maven = import ./maven.nix;
  request = {
    repositories = [ "https://example.invalid/maven" ];
    resolutionContext.systemProperties."os.arch" = "amd64";
    classpaths.main = {
      dependencies = [ "example:library:1" ];
      scope = "compile";
    };
  };
  declared =
    { maven }:
    request
    // {
      classpaths.main = maven.resolve {
        dependencies = [ "example:library:1" ];
        scope = "compile";
      };
    };
  normalize = spec: (java.dependencySpec spec).spec;
  matrix = spec: {
    inherit (spec) repositories classpaths;
    environments.linux.resolutionContext.systemProperties."os.arch" = "amd64";
  };
  workspace = import ./workspace-dependencies.nix {
    inherit pkgs;
    dependencyApi = java;
  };
  fixture = ../examples/hello/src;
  core = java.library {
    name = "core";
    sources = fixture;
    compile = { };
    runtime = { };
  };
  producer = java.library {
    name = "producer";
    sources = fixture;
    compile.modulepath = [ core ];
    runtime.modulepath = [ core ];
  };
  consumer = java.library {
    name = "consumer";
    sources = fixture;
    compile.classpath = [
      producer
      producer
    ];
  };
  legacy = java.library {
    name = "legacy";
    sources = fixture;
    dependencies = [ producer ];
  };
  app = java.application {
    name = "app";
    runtime.classpath = [ producer ];
    mainClass = "example.Main";
  };
  bad = value: !(builtins.tryEval (builtins.deepSeq value true)).success;
  invalidMixed = java.library {
    name = "mixed";
    sources = fixture;
    compile = { };
    dependencies = [ ];
  };
  invalidPlacement = java.library {
    name = "placement";
    sources = fixture;
    compile = {
      classpath = [ core ];
      modulepath = [ core ];
    };
  };
  invalidKeys = java.library {
    name = "keys";
    sources = fixture;
    runtime.unexpected = [ ];
  };
  conflicting = core // {
    jixLibrary = core.jixLibrary // {
      entry = core.jixLibrary.entry // {
        path = "/different.jar";
      };
    };
  };
  conflict = java.library {
    name = "conflict";
    sources = fixture;
    compile.classpath = [
      core
      conflicting
    ];
  };
  ids = entries: map (e: e.id) entries;
  empty = {
    classpath = [ ];
    modulepath = [ ];
  };
in
assert normalize declared == normalize request;
assert (java.dependencySpec declared).specHash == (java.dependencySpec request).specHash;
assert
  normalize (
    { maven }:
    matrix (declared {
      inherit maven;
    })
  ) == normalize (matrix request);
assert
  (workspace {
    specs.app = declared;
    lockFile = ../examples/hello/absent.lock;
  }).descriptor.spec == (workspace {
    specs.app = request;
    lockFile = ../examples/hello/absent.lock;
  }).descriptor.spec;
assert consumer.jixLibrary.runtime == empty;
assert ids consumer.jixLibrary.compile.classpath == [ "producer" ];
assert consumer.jixLibrary.compile.modulepath == [ ];
assert ids producer.jixLibrary.compile.modulepath == [ "core" ];
assert ids legacy.jixLibrary.compile.modulepath == [ "core" ];
assert ids app.jixApplication.runtime.classpath == [ "producer" ];
assert app.jixApplication.runtime.modulepath == [ ];
assert
  (builtins.head consumer.jixLibrary.ide.libraries).jixLibrary.compile == producer.jixLibrary.compile;
assert bad invalidMixed;
assert bad invalidPlacement.jixLibrary.compile;
assert bad invalidKeys.jixLibrary.runtime;
assert bad conflict.jixLibrary.compile;
assert bad (
  maven.resolve {
    dependencies = [ ];
    scope = "invalid";
  }
);
{
  explicit-inputs = pkgs.runCommand "jix-explicit-inputs-tests" { } ''touch "$out"'';
}
