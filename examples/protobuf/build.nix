{
  pkgs,
  java,
  deps,
}:
let
  protoc = import ./protoc.nix { inherit pkgs; };
  generatedJava = pkgs.runCommand "protobuf-java-sources" { nativeBuildInputs = [ protoc ]; } ''
    mkdir -p "$out/java"
    protoc --proto_path=${./src/main/proto} --java_out="$out/java" ${./src/main/proto}/message.proto
  '';
  runtime = {
    modulepath = [ deps.classpaths.main ];
  };
  jar = java.library {
    name = "application";
    sources = [
      ./src/main/java
      "${generatedJava}/java"
    ];
    compile.modulepath = [ deps.classpaths.main ];
    inherit runtime;
    jdk = pkgs.jdk21;
    release = 21;
  };
  app = java.application {
    name = "application";
    runtime = runtime // {
      modulepath = [ jar ] ++ runtime.modulepath;
    };
    module = "example.protobuf/example.Main";
    jdk = pkgs.jdk21;
  };
in
{
  inherit
    jar
    app
    generatedJava
    protoc
    ;
}
