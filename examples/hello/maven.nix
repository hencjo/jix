{ maven }:
let
  common = [ "org.junit.platform:junit-platform-commons:1.11.4" ];
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
