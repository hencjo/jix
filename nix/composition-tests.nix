{ pkgs, java }:
let
  workspace = import ./workspace-dependencies.nix {
    inherit pkgs;
    dependencyApi = java;
  };
  fixture = ../examples/monorepo;
  recipe = import (fixture + "/build-support/parser/default.nix");
  deps = workspace {
    specs.parser = recipe.spec;
    lockFile = fixture + "/jix.lock";
  };
  build = import (fixture + "/build.nix") { inherit java deps; };
  independent = java.importLibrary {
    descriptor = fixture + "/build-support/parser/default.nix";
    src = fixture + "/vendor/parser";
  };
  bad = value: !(builtins.tryEval value).success;
  conflicting = build.parser // {
    jixLibrary = build.parser.jixLibrary // {
      entry = build.parser.jixLibrary.entry // {
        path = "${build.unrelated}";
      };
    };
  };
  dual = java.library {
    name = "dual";
    sources = fixture + "/applications/plain/src";
    dependencies = [ build.parser ];
    moduleDependencies = [ build.parser ];
  };
  conflict = java.library {
    name = "conflict";
    sources = fixture + "/applications/plain/src";
    dependencies = [
      build.parser
      conflicting
    ];
  };
  mavenView = version: {
    jixEntries = [
      {
        id = "fixture:thirdparty:jar::${version}";
        key = "fixture:thirdparty:jar:";
        inherit version;
        path = "${pkgs.writeText "thirdparty-${version}.jar" version}";
      }
    ];
  };
  mavenConflict = java.library {
    name = "maven-conflict";
    sources = fixture + "/applications/plain/src";
    dependencies = [
      (mavenView "1")
      (mavenView "2")
    ];
  };
  override = workspace {
    lockFile = fixture + "/absent.lock";
    repositories = [ "https://example.invalid/maven" ];
    specs = {
      a = recipe.spec // {
        repositories = [ "https://a.invalid" ];
      };
      b = recipe.spec // {
        repositories = [ "https://b.invalid" ];
      };
    };
  };
in
{
  library-composition =
    assert independent.drvPath == build.parser.drvPath;
    assert bad dual.drvPath;
    assert bad conflict.drvPath;
    assert bad mavenConflict.drvPath;
    assert bad (
      (workspace {
        lockFile = fixture + "/absent.lock";
        specs = {
          a = recipe.spec // {
            repositories = [ "https://a.invalid" ];
          };
          b = recipe.spec // {
            repositories = [ "https://b.invalid" ];
          };
        };
      }).descriptor.spec.repositories
    );
    assert override.descriptor.spec.repositories == [ "https://example.invalid/maven" ];
    assert
      builtins.attrNames override.descriptor.spec.classpaths == [
        "a.main"
        "b.main"
      ];
    pkgs.runCommand "jix-library-composition" { nativeBuildInputs = [ pkgs.jdk21 ]; } ''
      ${build.application}/bin/server | grep -Fx composed
      ${build.plainApp}/bin/plain | grep -Fx composed
      ${build.mixedApp}/bin/mixed | grep -Fx composed
      jar --describe-module --file ${build.core} | grep -F com.example.core
      test -f ${build.unrelated}
      touch "$out"
    '';
}
