{ pkgs, jix }:
let
  project = import ./project.nix;
  hello = import ../examples/hello { inherit pkgs jix; };
  duplicateClasses = jix.compileJava {
    name = "hello-classes-composed";
    sources = ../examples/hello/src/main/java;
    classpaths = [
      hello.classpaths.main
      hello.classpaths.runtime
    ];
  };
  resourceVariant = jix.jar {
    name = "hello-variant";
    classes = hello.classes;
    resources = [
      {
        from = pkgs.writeText "message.txt" "Different resource\n";
        into = "message.txt";
      }
    ];
    manifest."Main-Class" = "example.Main";
  };
  identicalJar = jix.jar {
    name = "hello-repeated";
    classes = hello.classes;
    resources = [
      {
        from = ../examples/hello/src/main/resources;
        into = "";
      }
    ];
    manifest."Main-Class" = "example.Main";
  };
  rawRequest = (jix.dependencySpec (import ../examples/hello/maven.nix)).spec;
  changedRequest = jix.dependencySpec (
    rawRequest
    // {
      classpaths = rawRequest.classpaths // {
        runtime = {
          dependencies = [ ];
        };
      };
    }
  );
  stale = builtins.tryEval (
    (jix.lockedClasspaths {
      dependencies = changedRequest;
      lockFile = ../examples/hello/jix.lock;
    }).main.drvPath
  );
in
{
  hello = pkgs.runCommand "jix-hello-test" { nativeBuildInputs = [ pkgs.jdk21 ]; } ''
    ${hello.launcher}/bin/jix-hello | grep -Fx 'Hello from a locked, offline Java build!'
    cmp ${hello.app} ${identicalJar}
    if cmp -s ${hello.app} ${resourceVariant}; then echo 'resource change did not change JAR' >&2; exit 1; fi
    diff -r ${hello.classes} ${duplicateClasses}
    touch "$out"
  '';
  project-validation =
    let
      deps = hello.dependencies;
      make = args: project ({ inherit pkgs deps; } // args);
      bad = value: !(builtins.tryEval value).success;
      normal = make {
        targets.application = hello.launcher;
        defaultTarget = "application";
      };
    in
    assert normal.packages.default.drvPath == normal.descriptor.targets.application.drvPath;
    assert
      normal.packages.classpath-main.drvPath == normal.descriptor.targets."classpaths.main".drvPath;
    assert bad ((make { targets.dependencies = hello.launcher; }).packages.default.drvPath);
    assert bad ((make { targets.classpath-main = hello.launcher; }).packages.default.drvPath);
    assert bad ((make { defaultTarget = "missing"; }).packages.default.drvPath);
    assert bad ((make { defaultApp = "missing"; }).apps.default.program);
    assert bad (
      (make {
        apps.bad = {
          type = "wrong";
          program = "bad";
        };
      }).apps.bad.program
    );
    pkgs.runCommand "jix-project-validation" { } ''touch "$out"'';
  request-validation =
    assert !stale.success;
    pkgs.runCommand "jix-request-validation" { } ''touch "$out"'';
}
