# Jix

**Java dependencies, pinned. Java builds, powered by Nix.**

Declare Maven dependencies once. Lock their graphs and hashes. Let Nix fetch,
verify, cache, and retain the artifacts—for your existing build or Jix's optional
Java build DSL.

- **Start small.** Use locked classpaths without adopting a new build system.
- **Build from pins.** Builds never resolve dependencies or update locks.
- **Keep inputs explicit.** Pin Maven profile contexts as well as toolchains.
- **Use ordinary Nix.** Flake packages, apps, and binary caches work as usual.

## Build your first JAR

You need Nix and Jix on Linux (x86-64 or AArch64).
[Building Jix from this checkout?](#develop-jix)

### 1. Create a project

```sh
mkdir hello-jix && cd hello-jix
jix init --template fat
```

This generates `flake.nix`, `build.nix`, and a Java source tree. Keep the generated
build recipe: it compiles with Java 21 and packages the runtime dependencies into
an executable JAR. It also captures Maven profile inputs into
`resolution-context.nix`, so you do not have to write them by hand.

### 2. Add a dependency

Replace `maven.nix` with:

```nix
{ maven }:
let
  dependencies = [ "org.apache.commons:commons-lang3:3.17.0" ];
in {
  resolutionContext = import ./resolution-context.nix;
  classpaths = {
    main = maven.resolve { scope = "compile"; inherit dependencies; };
    runtime = maven.resolve { scope = "runtime"; inherit dependencies; };
  };
}
```

Replace `src/main/java/example/Main.java` with:

```java
package example;

import org.apache.commons.lang3.StringUtils;

public final class Main {
    public static void main(String[] args) {
        System.out.println(StringUtils.capitalize("hello from Jix!"));
    }
}
```

### 3. Lock. Build. Run.

```sh
jix lock                 # fetch metadata and pin dependency artifacts
jix build jar            # print the executable JAR's Nix store path
jix run                  # Hello from Jix! (uses the pinned JDK)
```

The JAR includes Commons Lang; it can also run with `java -jar /path/to/the.jar`
on Java 21 or newer. Commit the generated project, including both `flake.lock`
and `jix.lock`, plus `resolution-context.nix`, to preserve the toolchain, dependency,
and Maven profile inputs.

## Keep your build. Use pinned dependencies.

You do not need the Java build DSL to use Jix. Start with dependency management:

```sh
jix init                         # create a dependency-only project; needs PATH Java
# Add dependencies to maven.nix
jix lock                         # resolve and pin
jix build dependencies           # fetch artifacts; no Java compilation
jix classpath main               # print the usable classpath
jix export --output ./deps       # copy dependencies into a portable directory
jix verify                       # check declaration/lock agreement offline
```

Maven Central is the default. Declare separate compile, runtime, and test
classpaths with `maven.resolve`; use BOMs and exclusions where needed.
Jix preserves dependency order and checks its supported resolution behavior
against Maven 3.9.11 / Resolver 1.9.24.

[Declare dependencies →](docs/guide.md#declare-dependencies-once) ·
[Verify and export →](docs/guide.md#verify-and-export)

## Two locks. No surprises.

| Input | Pinned in |
| --- | --- |
| Nix packages and toolchains | `flake.lock` |
| Maven selections, artifact hashes, and resolution contexts | `jix.lock` |

`jix init` captures Java/OS properties once. Applications use their pinned build
JDK; dependency-only projects use `java` on PATH. Review and commit the generated
`resolution-context.nix`: `java.home` may contain a local path, but it is metadata,
not a tool lookup.

Changing machines does not change these inputs. Edit declarations and run
`jix lock` to update Maven pins; use `nix flake update` to update Nix inputs.
Missing or stale locks stop the build instead of resolving behind your back.

## Several environments. One lock.

Declare named environments when dependencies need different Java/OS inputs.
`jix lock` pins **every environment**, not just the one you are using.

```sh
jix lock
jix build --environment linux-amd64
jix build --environment linux-aarch64
```

For a matrix project, selection is `--environment`, then `JIX_ENVIRONMENT`, then
`defaultEnvironment`. Without a selection, Jix stops and lists the choices.
Single-context projects always use their one captured context.

Environment selection chooses a frozen dependency graph. It does not configure
cross-compilation or make another architecture's native libraries executable.

[Declare an environment matrix →](docs/guide.md#lock-several-named-environments)

## Pick your artifact.

```sh
jix init --template fat
```

| Template | What you get |
| --- | --- |
| `dependencies` (default) | Locked classpaths; keep your own build |
| `thin` (`java` alias) | A library JAR and a Nix-backed launcher |
| `fat` | An executable JAR with its runtime dependencies |
| `shadow` | A fat JAR with explicit package relocation |
| `multi-module` | Separate core and application libraries |

Thin JARs need their launcher; they are not standalone distributions.
Compile and runtime inputs are explicit and independent—runtime never silently
inherits compilation dependencies.

[Java build DSL →](docs/guide.md#optional-java-build-dsl) ·
[Testing with Nix →](docs/guide.md#testing-with-nix) ·
[Working templates →](templates/)

## Same project. Native Nix outputs.

Generated applications work with both interfaces:

| Jix | Nix |
| --- | --- |
| `jix build jar` | `nix build .#jar` |
| `jix build dependencies` | `nix build .#dependencies` |
| `jix run` | `nix run` |

Dependencies are individual hash-checked Nix downloads. Standard binary caches
work without a separate Jix cache. Other flakes can consume your packages directly.

[Integrate with an existing flake →](docs/guide.md#one-project-native-flake-outputs)

## Open IntelliJ. Build and debug.

```sh
jix idea
jix idea --open
```

Jix realizes dependencies and JDKs, then generates native repository-local IntelliJ
configuration. Register the printed SDK once. Rerun after dependency or build-model
changes; Jix never edits global IDE settings or `.idea/workspace.xml`.

IDE output is for development; Nix builds remain authoritative. Unsupported IDE
layouts fail explicitly rather than producing a misleading configuration.

[IntelliJ setup and limits →](docs/guide.md#intellij)

## Migrate with your coding agent.

Give your agent this instruction:

> Read `jix migrate --guide`, inspect this project, and help migrate it to Jix.
> Keep the existing build until verification passes.

The guide ships in the binary. It covers Maven/Gradle assessment, dependency-only
adoption, libraries, workspaces, JPMS, and verification—not automatic conversion.

[Read the migration guide →](crates/jix/docs/migration.md)

## Go deeper.

| Guide | What it covers |
| --- | --- |
| [User guide](docs/guide.md) | Declarations, matrices, build APIs, testing, verification, and exports |
| [Compatibility](COMPATIBILITY.md) | Supported Maven semantics and explicit limitations |
| [Lock format](docs/lock-format.md) | Lock v5 and upgrading older projects |
| [Monorepo example](examples/monorepo) | Modules, local libraries, and a vendored adapter |
| [Generated sources](examples/protobuf/README.md) | Pinned protoc and build-time Java generation |

Jix is not a Maven/Gradle lifecycle runner. It does not execute their plugins,
implement annotation processing, or publish Maven artifacts. Linux x86-64 and
AArch64 packages are declared; host validation runs on x86-64.

## Develop Jix

From this checkout:

```sh
nix build .#jix
./result/bin/jix --help

# Pinned development tools and the full release gate:
devenv shell
bash scripts/check-release
```

Use [Conventional Commits](CONTRIBUTING.md#changelog-and-versions) and keep
user-facing changes under Unreleased. Releases run locally—not through release PRs.

[Contributing and releases →](CONTRIBUTING.md) ·
[Changelog →](CHANGELOG.md)

## License

GPL-3.0-or-later. See [LICENSE](LICENSE). Third-party fixtures and bundled
components retain their own licenses.
