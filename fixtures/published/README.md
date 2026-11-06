# Frozen published-POM corpus

Unmodified POMs downloaded from Maven Central on 2026-09-11. Each file's
repository-relative path appended to `https://repo.maven.apache.org/maven2/`
is its source URL. `SHA256SUMS` pins the bytes; verify from this directory:

```sh
sha256sum --check SHA256SUMS
```

The corpus contains JUnit Jupiter API 5.11.4, its BOM and its dependency closure,
plus Reactive Streams 1.0.4 and javax.inject 1.
Upstream license declarations and attribution remain in the original POM files.
JUnit POMs declare EPL-2.0; API Guardian and OpenTest4J declare Apache-2.0.

Differential tests copy this metadata into an isolated repository and create
**synthetic empty JAR placeholders**. The placeholders are not upstream binaries:
this corpus tests resolution, not Java compilation or binary integrity.
