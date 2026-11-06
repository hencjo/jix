#!/usr/bin/env bash
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
home=${JIX_ORACLE_HOME:-$root/target/oracle}
exec java -Dorg.slf4j.simpleLogger.defaultLogLevel=error -cp "$home:$home/apache-maven-3.9.11/lib/*:$home/apache-maven-3.9.11/boot/*" Oracle "$@"
