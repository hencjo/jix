{ pkgs }:
let
  version = "9.10.1";
  hashes = {
    asm = "ed825d10ab1399c8c0cb669e688cf0c8c82629b4c8399b58352b68e92ca10fcb";
    asm-commons = "6d0abefb7cbf972ea16edb37ec14835372505063a45f976ab7ea889ed9497895";
    asm-tree = "3dfb0d5b6a106cd40b5b250e39935fbf2f927f4477546a5369a3ac609cf0506b";
  };
  jars = pkgs.lib.mapAttrsToList (
    name: hash:
    pkgs.fetchurl {
      url = "https://repo.maven.apache.org/maven2/org/ow2/asm/${name}/${version}/${name}-${version}.jar";
      sha256 = hash;
    }
  ) hashes;
  classpath = pkgs.lib.concatStringsSep ":" (map toString jars);
  classes = pkgs.runCommand "jix-relocator-classes" { nativeBuildInputs = [ pkgs.jdk21 ]; } ''
    mkdir "$out"
    cp ${../tools/relocator/Relocate.java} Relocate.java
    javac --release 21 -proc:none -cp ${classpath} -d "$out" Relocate.java
    cp ${../tools/relocator/ASM-LICENSE} "$out/ASM-LICENSE"
  '';
in
pkgs.writeShellApplication {
  name = "jix-relocate";
  text = ''exec ${pkgs.jdk21}/bin/java -cp ${classes}:${classpath} jix.Relocate "$@"'';
}
