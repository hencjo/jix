{ pkgs, java }:
let
  thin = java.library {
    name = "relocation-fixture";
    sources = ../fixtures/packaging/src;
    resources = [ { from = ../fixtures/packaging/resources; } ];
  };
  fat =
    name: strings:
    java.fatJar {
      inherit name;
      dependencies = [ thin ];
      mainClass = "example.Main";
      relocations = [
        {
          from = "example";
          to = "internal.example";
        }
      ];
      relocateStringConstants = strings;
    };
  first = fat "first" true;
  second = fat "second" true;
  literal = fat "literal" false;
in
{
  packaging = pkgs.runCommand "jix-packaging-tests" { nativeBuildInputs = [ pkgs.jdk21 ]; } ''
    cmp ${first} ${second}
    java -jar ${first} > result
    echo 'relocated:resource' > expected
    cmp result expected
    if java -jar ${literal} > literal.log 2>&1; then
      echo "Reflection strings were unexpectedly relocated by default" >&2
      exit 1
    fi
    grep 'ClassNotFoundException: example.Main' literal.log
    touch "$out"
  '';
}
