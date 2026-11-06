#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."
version=3.9.11
sha=4b7195b6a4f5c81af4c0212677a32ee8143643401bc6e1e8412e6b06ea82beac
mkdir -p target/oracle
archive=target/oracle/apache-maven-$version-bin.tar.gz
if [[ ! -f "$archive" ]]; then
  curl --fail --location --retry 2 "https://repo.maven.apache.org/maven2/org/apache/maven/apache-maven/$version/apache-maven-$version-bin.tar.gz" -o "$archive.tmp"
  mv "$archive.tmp" "$archive"
fi
printf '%s  %s\n' "$sha" "$archive" | sha256sum --check
# Always extract the verified distribution; never trust an old extracted classpath.
tar -xzf "$archive" -C target/oracle
javac --release 21 -cp "target/oracle/apache-maven-$version/lib/*:target/oracle/apache-maven-$version/boot/*" -d target/oracle tools/oracle/Oracle.java
