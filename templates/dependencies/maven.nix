{ maven }:
# Declare exact releases here; share lists using ordinary Nix expressions.
let
  common = [ ];
in
{
  classpaths = {
    main = maven.resolve {
      scope = "compile";
      dependencies = common;
    };
    runtime = maven.resolve {
      scope = "runtime";
      dependencies = common;
    };
  };
}
