{ pkgs }:
if pkgs.stdenv.buildPlatform.system != "x86_64-linux" then
  throw "protobuf example: protoc 4.33.4 is pinned only for x86_64-linux"
else
  pkgs.runCommand "protoc-4.33.4"
    {
      src = pkgs.fetchurl {
        url = "https://repo.maven.apache.org/maven2/com/google/protobuf/protoc/4.33.4/protoc-4.33.4-linux-x86_64.exe";
        sha256 = "72e5311d8f2faaea1fcb15d5475ac5f5742ec6dbba8491bb473fc060e4e29e8f";
      };
    }
    ''
      mkdir -p "$out/bin"
      # The published Linux executable is static; no host loader or patching needed.
      install -m755 "$src" "$out/bin/protoc"
      test "$("$out/bin/protoc" --version)" = "libprotoc 33.4"
    ''
