{ pkgs }:

pkgs.stdenvNoCC.mkDerivation {
  pname = "jix-maven-oracle";
  version = "3.9.11";
  src = pkgs.fetchurl {
    url = "https://repo.maven.apache.org/maven2/org/apache/maven/apache-maven/3.9.11/apache-maven-3.9.11-bin.tar.gz";
    sha256 = "4b7195b6a4f5c81af4c0212677a32ee8143643401bc6e1e8412e6b06ea82beac";
  };
  dontUnpack = true;
  nativeBuildInputs = [ pkgs.jdk21 ];
  installPhase = ''
    mkdir -p "$out"
    tar -xzf "$src" -C "$out"
    cp ${../tools/oracle/Oracle.java} Oracle.java
    javac --release 21 \
      -cp "$out/apache-maven-3.9.11/lib/*:$out/apache-maven-3.9.11/boot/*" \
      -d "$out" Oracle.java
  '';
}
