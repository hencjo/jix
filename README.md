# Jix

**Nix-backed Java dependencies, independent of your build tool.** Rust resolves
published Maven metadata; upstream Nix fetches, verifies, caches, and retains the
locked artifacts. An optional Java build DSL consumes the same classpaths.

## Migrate with your coding agent

Ask your existing coding agent:

> Read `jix migrate --guide`, inspect this project, and help migrate it to Jix.
> Keep the existing build until verification passes.

The guide covers Maven/Gradle assessment, dependency-only adoption, libraries, workspaces, vendored adapters,
JPMS, and evidence-based verification; it does not promise automatic conversion.

## Start a project

With Jix installed:

```sh
jix init                         # dependencies-only project
# Edit maven.nix
jix lock                         # resolve and pin Maven dependencies
jix build dependencies           # populate the store; no Java compilation
jix classpath main               # print the ordered, usable classpath
jix export --output ./deps       # export portable copies for other tools
jix verify                       # check lock freshness offline
```

For an application, start with `jix init --template java`, then `jix lock`, `jix build`, and
`jix run` (or `nix run`). This adds a separate `build.nix`, Java sources, and resources.

### Templates

`jix init --template NAME` embeds the scaffolding in the binary:

| Template | Result |
| --- | --- |
| `dependencies` (default) | Reusable locked classpaths, no build DSL |
| `thin` (`java` alias) | Library JAR and Nix-store-backed launcher |
| `fat` | Executable JAR containing its runtime classpath |
| `shadow` | Fat JAR with explicit package relocation |
| `multi-module` | Separate core/application source libraries |

Then `jix lock`, `jix run`, or `jix build jar`. Every application template
exports ordinary flake packages and apps; `nix build .#jar` and `nix run` work.
Thin JARs are not standalone distributions: use their Nix launcher.

### IntelliJ

```sh
jix idea               # realize dependencies/JDKs, generate repository .idea/
jix idea --open        # also launch an installed idea executable
```

Register each printed SDK name and store home once in IntelliJ's SDK settings,
then open the directory. Jix never edits global IDE settings. Generated native
Application configurations support Build/Run/Debug of local sources, with output
in `.jix/idea-out/`; they do not run the fat/relocated Nix artifact.

Regenerate after changing dependencies or the build model. Jix owns only files
recorded in `.jix/idea-state.json`; unmanaged collisions and edited generated
files fail rather than overwrite. `.idea/workspace.xml` is never touched.
Existing Maven/Gradle IDE configuration must be migrated explicitly.
Keep `.jix/` ignored; generated `.idea/` contains store-specific SDK references
and is ignored by new project templates. Embedded evaluation excludes both directories.

Supported: disjoint local source/resource roots, library dependencies (including
external store JARs), per-module release/JDK/compiler options and named Java
modules. Resource destination remapping, overlapping roots, application-only
dependencies, and explicit module-path placement for unnamed source/applications
are rejected rather than silently changed. Annotation processing remains disabled.
Nix builds remain authoritative; IDE output is for development.

From this checkout:

```sh
nix run 'path:.#jix' -- init --project /tmp/my-java --template java
nix run 'path:.#jix' -- lock --project /tmp/my-java
nix run 'path:/tmp/my-java'
```

Initialization never overwrites an existing project or stages files. Existing
flakes receive an integration snippet. `--jix-source FLAKE` selects the Jix input;
by default a Nix-packaged binary uses its own immutable source snapshot. For a
shared project, specify your published Git flake reference rather than a local
checkout/store path. `flake.lock` pins Nix inputs; `jix.lock` pins Maven inputs.
Ordinary operations never update either implicitly.

## Declare dependencies once

```nix
# maven.nix
{ maven }:
let
  common = [ "org.junit.platform:junit-platform-commons:1.11.4" ];
in {
  # Maven Central is the default. Repository and dependency order matter.
  classpaths = {
    main = maven.resolve { scope = "compile"; dependencies = common; };
    runtime = maven.resolve { scope = "runtime"; dependencies = common; };
    test = maven.resolve {
      scope = "test";
      platforms = [ "org.junit:junit-bom:5.11.4" ];
      dependencies = [
        "org.junit.jupiter:junit-jupiter-api"
        {
          module = "org.junit.jupiter:junit-jupiter-params";
          exclude = [ "org.apiguardian:apiguardian-api" ];
        }
      ];
    };
  };
}
```

`maven.resolve` declares a Maven resolution; it performs no network access or builds.
Its `scope` argument is required. `jix lock` executes the declaration and pins the
result; using the resulting classpath never applies scope again.
Specifications may be ordinary attribute sets or functions receiving `{ maven; }`;
keep `spec = import ./maven.nix;` in either case. The pure constructor is
also available directly as `jix.maven.resolve`. Existing raw declarations/JSON keep
their historical defaults and normalize to the same lock representation.

Classpaths resolve independently: compose shared lists with ordinary Nix.
Coordinates are `group:artifact:version`; a BOM may supply an omitted version.
Rich declarations also accept `classifier` and `extension`. Scope is a final
compile/runtime/test classpath filter, not a scope applied to every root dependency.

### Reproducible Maven profile activation

Published dependencies with environment-activated profiles require a declared
context, never the machine running Jix:

```nix
resolutionContext = {
  systemProperties = {
    "java.version" = "21.0.12";
    "java.home" = "/jix/declared-jdk-21";
    "os.name" = "Linux";
    "os.arch" = "amd64";
    "os.version" = "6.6";
    "path.separator" = ":";
  };
  userProperties = { };
};
```

Put this alongside `classpaths` in `maven.nix`. These are resolution
inputs, not JDK selection or host detection; choose values matching your intended
Maven session. `java.home` above is a declared absolute metadata path, not a JDK
installation. Jix never loads tools from it. Unix/DOS family activation also needs
`path.separator`.

A classpath's `resolutionContext` **replaces the entire shared context**; use Nix
composition for intentional reuse. Missing maps default to empty. User properties
override system properties for published descriptors; POM properties retain Maven
interpolation precedence. Missing environment values required by an activator
fail explicitly. No ambient environment variables, home-directory settings, or
filesystem facts are imported.

JDK, OS and property conditions, default suppression and profile ordering follow
the pinned Maven oracle. Published POMs have no project directory: relative file
conditions are inactive; absolute filesystem lookups and Java-regex OS version
conditions fail explicitly rather than guessing. Maven's synthetic `packaging`
property and nested activation-property names are also unsupported. See COMPATIBILITY.md.

### Lock several named environments

Use a matrix when the same classpaths need different Maven evaluation inputs:

```nix
let
  environment = arch: {
    resolutionContext.systemProperties = {
      "java.version" = "21.0.12";
      "java.home" = "/jix/declared-jdk-21";
      "os.name" = "Linux";
      "os.arch" = arch;
      "os.version" = "6.6";
      "path.separator" = ":";
    };
  };
  common = [ "com.google.guava:guava:33.5.0-jre" ];
in {
  environments = {
    linux-amd64 = environment "amd64";
    linux-aarch64 = environment "aarch64";
  };
  classpaths = {
    main.dependencies = common;
    runtime = { scope = "runtime"; dependencies = common; };
  };
}
```

Every environment gets the shared classpaths unless it declares its own complete
`classpaths` map. Use ordinary Nix composition for environment-specific classifiers
or dependency lists; there is no second dependency DSL. Matrix contexts are complete:
do not also declare global or per-classpath `resolutionContext`. Context properties
are recorded in the lockfile—do not put secrets in them.

`jix lock` evaluates and locks **all environments** into one v5 `jix.lock`, sharing
artifact hashes and POM provenance. Changed environment/classpath pairs resolve;
unchanged pairs retain their pins offline. Adding/removing an environment never
silently chooses or relabels an existing graph. Inputs must be host-independent Nix
expressions; pin toolchain inputs in `flake.lock`, never probe the current kernel.

```sh
jix lock
jix info                              # all environments
jix info --environment linux-amd64
jix verify                            # complete lock/declaration agreement
jix build dependencies --environment linux-amd64
jix classpath main --environment linux-amd64
jix export --environment linux-amd64 --output ./exported
jix verify ./exported/jix.lock --environment linux-amd64 \
  --artifacts ./exported/artifacts.json
```

Matrix build/classpath/export/run/idea operations select an environment in this order:
`--environment NAME`, `JIX_ENVIRONMENT`, then project `defaultEnvironment`.
Missing or invalid selections fail with available names; an invalid override never
falls back. Legacy non-matrix projects ignore `JIX_ENVIRONMENT`.
Exporting an existing realized directory defaults to its validated manifest selection;
an explicit flag or variable must match it, and no Nix evaluation is needed.

Locking always covers the complete matrix. Lock/resolve/info/verify ignore the variable
and project default; info/verify retain explicit selection/filtering.
Defaults never enter dependency hashes or change Maven activation inputs.
Exported manifests identify the selected environment, context and lock fingerprint;
wrong selections fail verification.
Exports contain only that environment's bytes, alongside the complete matrix lock.

The Nix dependency API exposes `deps.environments.<name>.classpaths` and `.all`.
Matrix `jix.project` definitions can reuse a Java recipe through a callback:

```nix
jix.project {
  inherit pkgs deps;
  defaultEnvironment = {
    x86_64-linux = "linux-amd64";
    aarch64-linux = "linux-aarch64";
  }.${pkgs.stdenv.hostPlatform.system} or null;
  forEnvironment = { name, deps }:
    let build = import ./build.nix {
      inherit pkgs deps;
      java = jix.lib { inherit pkgs; };
    };
    in {
      targets.application = build.app;
      defaultTarget = "application";
      apps.application = { type = "app"; program = "${build.app}/bin/application"; };
      defaultApp = "application";
    };
}
```

Without a callback, matrix projects expose dependency/classpath targets only.
Flake packages/apps are qualified: `linux-amd64--dependencies`,
`linux-amd64--classpath-main`, `linux-amd64--application`, and `linux-amd64--default`.
A declared `defaultEnvironment` also exposes `packages.default` and, when the selected
project has a default app, `apps.default`. Thus `jix build`, `jix run`, `nix build`
and `nix run` work without a qualifier. `JIX_ENVIRONMENT` only affects Jix commands,
not pure flake outputs. Omit the default (or use `null`) to require a CLI selection.
For CI, prefer `--environment NAME` to override developer-shell preferences.
This selects pinned graphs; it neither probes Java/kernel properties nor configures
cross-compilation.

Environment names allow letters, digits,
`_` and `-`, but reserve `--` for qualification.

Workspace components must all use matrix declarations or all use legacy declarations.
Matrix components may participate in different environment subsets; matching names
must have identical contexts. Selected views retain
`deps.environments.<environment>.components.<component>.classpaths`.

This selects **dependency variants**, not cross-compilation toolchains or emulation.
Maven's declared Java version is not `javac --release`. Selecting ARM dependencies
on x86 does not make native libraries executable there. Existing unsupported Maven
features still fail explicitly. Single-context projects and their CLI remain unchanged;
Single and matrix locks both use v5; older lock versions require an explicit re-lock.

After adding a dependency, run `jix lock` again. Only changed/new classpaths
resolve; unchanged classpaths retain their pins without repository access. Removing
a classpath prunes unowned inputs. Changing repository order or URLs resolves
everything again. Conflicting shared inputs fail without replacing the lock.
A no-op leaves the file untouched.

`jix info` shows locked dependencies; `--classpath main` filters them and
`--json` emits metadata. It reads only the lock and does not claim freshness.

`jix build` builds the template default: dependencies for dependency-only projects,
application for Java projects. `jix build dependencies` never compiles Java;
`jix build classpaths.main` builds one classpath. Builds never update pins.
Use `nix flake update` explicitly for Nix inputs.
 Missing/stale locks fail with an
instruction to do that—builds never resolve dependencies behind your back.

## Use the Nix dependency module directly

```nix
deps = inputs.jix.dependencies {
  inherit pkgs;
  spec = import ./maven.nix;
  lockFile = ./jix.lock;
};
```

Expose these outputs from your flake:

```nix
jix.${system}.dependencies = deps.descriptor;
packages.${system} = {
  dependencies = deps.all;
  classpath-main = deps.classpaths.main;
};
```

- `deps.specFile`: generated JSON; evaluating it needs no Maven lock.
- `deps.all`: every locked JAR and recorded POM, artifact manifest, and classpaths.
- `deps.classpaths.<name>`: independently buildable classpath with `classpathFile`,
  `entriesFile`, `jixEntries`, and ordered `jars/` symlinks.
- `deps.descriptor`: v2 frontend interface with `spec`, `lockValid`,
  `defaultTarget`, and a flat `targets` map containing `dependencies` and
  `"classpaths.<name>"`. Extend the map with custom derivations and override
  `defaultTarget`. Version 1 descriptors remain supported.

`nix build .#dependencies` obtains dependencies without compilation.
`nix build .#classpath-main` obtains only that classpath's JARs. Each download is
an individual `pkgs.fetchurl` derivation with its locked hash; standard Nix caches
and Cachix work without another Jix cache.

`jix build classpaths.main` retains a separate project root. Roots live under
`.jix/roots/`; deleting one permits its unreferenced closure to be garbage-collected.
Initialization also roots immutable local Nix input snapshots so the default
bootstrap source survives GC. The aggregate references all its inputs. Project commands use filtered `path:`
snapshots, including untracked source files but excluding root `.git`, `.jix`,
`.devenv`, `.direnv`, `target`, and `result`. No ancestor-directory discovery:
use `--project DIR` to select a different project.

## One project, native flake outputs

Use `inputs.jix.project` to expose the same targets through Jix and standard Nix.
It needs only the dependency module—not the Java DSL:

```nix
# Within your per-system definition:
project = inputs.jix.project {
  inherit pkgs deps;
  targets = {
    application = build.app;
    jar = build.jar;
  };
  defaultTarget = "application"; # defaults to "dependencies"
  apps.application = {
    type = "app";
    program = "${build.app}/bin/application";
  };
  defaultApp = "application";   # omitted: no default app
};
```

Expose `project.packages`, `project.apps`, and `project.descriptor` as
`packages.${system}`, `apps.${system}`, and `jix.${system}.dependencies`.
[The templates](templates/) do this without duplicating target definitions.

| Jix | Nix |
| --- | --- |
| `jix build` | `nix build` |
| `jix build application` | `nix build .#application` |
| `jix build jar` | `nix build .#jar` |
| `jix build dependencies` | `nix build .#dependencies` |
| `jix build classpaths.main` | `nix build .#classpath-main` |
| `jix run` | `nix run` |
| `jix run application -- ARG` | `nix run .#application -- ARG` |

Downstream flakes consume `inputs.myProject.packages.${system}.jar` normally:
no Jix CLI invocation or generated secondary flake is needed. Standard Nix builds
and app programs check the Maven lock too. Existing projects adopt the helper
manually; `init` never rewrites them.

`jix run [APP] [--project DIR] -- [ARGS...]` realizes the declared store
executable through embedded Nix and retains its references under `.jix/roots/app-*`.
It executes directly, preserving arguments (including `--help` after `--`),
environment, caller working directory, terminal streams, and exit/signal behavior.
Dependency-only projects have no apps. No executable-name guessing or implicit
lock updates occur.

Target names reserve `default`, `dependencies`, generated `classpath-NAME`
packages, and the `classpaths.` namespace. App names reserve `default`; use
`defaultApp` to select it. Names use ASCII letters, digits, dots, underscores,
and hyphens, excluding empty names, `.`, and `..`.

## Testing with Nix

Jix supplies locked dependencies and, optionally, compiles test JARs. Nix wires
execution; your runner discovers tests. JUnit is only the example here—TestNG,
a Java main class or a shell integration test can use the same checks/apps pattern.

Add a test resolution to `maven.nix` (reuse your application's Maven dependency
list here too if tests need it):

```nix
# Inside the { maven }: declaration:
classpaths.test = maven.resolve {
  scope = "test";
  dependencies = [
    "org.junit.jupiter:junit-jupiter:5.10.2"
    "org.junit.platform:junit-platform-console:1.10.2"
  ];
};
```

Run `jix lock` after editing the declarations. In `build.nix`, alongside your
application library `jar`, declare the complete test inputs explicitly:

```nix
# Within the existing let, with pkgs, java, deps and jar in scope:
testInputs.classpath = [ jar deps.classpaths.test ];
testJar = java.library {
  name = "unit-tests";
  sources = ./src/test/java;
  compile = testInputs;
};
testRunner = java.application {
  name = "unit-tests";
  mainClass = "org.junit.platform.console.ConsoleLauncher";
  runtime.classpath = [ testJar ] ++ testInputs.classpath;
};
testCheck = pkgs.runCommand "unit-tests-check" { } ''
  mkdir -p "$out"
  ${testRunner}/bin/unit-tests execute \
    --scan-classpath=${testJar.jarFile} \
    --fail-if-no-tests --reports-dir="$out"
'';
```

The check passes the test JAR explicitly: JUnit’s default classpath scan does not
scan JARs merely present on the JVM classpath.

Export `testRunner` and `testCheck` from `build.nix` along with your other
targets. Local library references contribute only their own JAR: explicitly add
any other required local libraries and Maven dependencies to `testInputs`.

In your flake outputs, where `system` and `build` are in scope:

```nix
checks.${system}.unit = build.testCheck;
apps.${system}.unit = {
  type = "app";
  program = "${build.testRunner}/bin/unit-tests";
};
```

Merge these with your existing outputs. With `jix.project`, put `apps.unit`
in its `apps` argument instead, and expose `checks` directly from the flake;
the helper does not accept a `checks` argument.

```sh
nix flake check -L                          # All checks; successful results may be cached
nix build .#checks.x86_64-linux.unit -L      # One check; reports under result/
nix run .#unit -- execute --select-class example.TokenTest --fail-if-no-tests
```

Replace `x86_64-linux` with your Nix system and `example.TokenTest` with your
test class. `nix run` executes tests fresh, outside the build sandbox; arguments go straight to JUnit. A failed test
fails the check or returns a nonzero runner exit status. Sandboxed checks should
not depend on network services or secrets. Other frameworks replace the runner,
selectors and report options—not the Nix integration. There is no `jix test`
command or framework-specific discovery in Jix.

## Verify and export

```sh
jix verify                                   # declarations + lock structure
jix verify jix.lock --artifacts ./deps/artifacts.json
jix verify jix.lock --artifacts ./deps/artifacts.json --classpath main
jix verify jix.lock --dependencies dependencies.json
jix export ./realized-output --output ./deps
```

Verification checks required bytes against SHA-256, reports missing/corrupt files,
tolerates unrelated files, exits nonzero on failure, and never repairs files.
Project freshness evaluation is offline: pinned Nix inputs must already be cached.
Import-from-derivation is disabled; Linux syscall filtering prevents network
sockets and external fetch helpers during offline evaluation. Explicit lock/JSON
verification doesn't evaluate Nix at all.

Exports contain regular files in `repository/` using Maven repository paths,
`jix.lock`, `artifacts.json`, and relative classpath configurations. Move the export
freely; regenerate absolute classpaths at its destination:

```sh
jix internal classpath ./deps/classpaths/main/classpath.json --output ./main-cp
cat ./main-cp/classpath
```

Exports verify bytes while copying and publish atomically to a new directory.
They include locked JARs/POMs, not unrequested sources/Javadoc. Maven-layout files
alone do not force Maven or Gradle to use Jix's exact dependency selection; those
tools need an explicit adapter for that guarantee.

## Optional Java build DSL

`inputs.jix.lib { inherit pkgs; }` exposes `compileJava` and `jar`; see
[the application template](templates/java/build.nix). The dependency module never
imports that build DSL. Resource edits do not invalidate compilation. JARs use
sorted entries, fixed timestamps/permissions, and reject collisions or unsafe paths.

### Complete compile and runtime inputs

`java.library` accepts complete, ordered input sets. Reuse values with ordinary Nix:

```nix
let
  runtime = { classpath = [ core deps.classpaths.runtime ]; };
  jar = java.library {
    name = "application";
    sources = ./src/main/java;
    compile.classpath = [ core deps.classpaths.main ];
    inherit runtime;
  };
  app = java.application {
    name = "application";
    runtime = runtime // { classpath = [ jar ] ++ runtime.classpath; };
    mainClass = "example.Main";
  };
in { inherit jar app; }
```

Each block supports `classpath` and `modulepath`; missing lists/blocks are empty.
Runtime does **not** inherit compilation inputs. A library reference contributes
only its own JAR; a concrete resolved classpath contributes its selected entries.
List all required local JARs explicitly. The producer keeps its original compilation
and IDE metadata even when consumed on a different path. The application JAR must
also be explicitly included in its launcher's runtime.

Identical entries deduplicate in declaration order; conflicting versions or placement
on both paths fail. Do not mix this form with legacy `dependencies`,
`moduleDependencies`, `runtimeDependencies` or `runtimeModuleDependencies`,
even when those legacy lists are empty. Legacy calls retain transitive propagation.

The DSL invokes the same offline Rust operations available explicitly:

```sh
jix internal classpath classpath.json --output ./classpath
jix internal compile compile.json --output ./classes
jix internal jar jar.json --output ./app.jar
```

Config-relative paths resolve relative to the JSON file. Outputs must not exist.
Classpath configs contain `entries` (`id`, `key`, `version`, `path`); compile configs
contain `sources`, explicit `javac`, `release`, and optional `classpaths` (entries
files) / `javacOptions`; JAR configs contain `classes`, optional `resources`
(`from`, `into`) and `manifest`. Unknown fields fail. These commands never call Nix
or access repositories. The `jix-build` Nix package excludes the embedded backend.


## Libraries, workspaces, and vendored sources

Set **java = inputs.jix.lib { inherit pkgs; };**. The following is the retained
**legacy transitive API**; prefer complete input sets above for explicit control:

~~~nix
core = java.library {
  name = "com.example.core"; # Stable identity, not a Maven coordinate
  sources = ./libraries/core/src;
  dependencies = [ deps.classpaths.main ];
  runtimeDependencies = [ plugin ];
  resources = [ { from = ./libraries/core/resources; } ];
};
server = java.library {
  name = "com.example.server";
  sources = ./applications/server/src;
  dependencies = [ core ]; # Or inputs.other.packages.${system}.core
};
app = java.application {
  name = "server";
  dependencies = [ server ];
  mainClass = "server.Main";
};
~~~

Libraries remain JAR derivations: export them directly as targets or flake packages.
Their **jixLibrary.version = 1** interface carries the own-JAR entry and ordered
**compile**/**runtime** closures, each split into **classpath** and **modulepath**.
Exporting only the JAR path loses that interface.

Compile dependencies propagate to consumers' compilation and runtime; additional
runtime dependencies propagate only at runtime. Identical inputs are deduplicated
in first-declaration order. Conflicting identities, versions, paths, or placement
fail: independently pinned libraries are never silently re-resolved together.
Use globally distinct library names and an acyclic dependency graph. Maven pins
stay in jix.lock; source libraries are pinned by source control or flake.lock.
The export command still exports Maven inputs, not an application distribution.

### Executable and relocated JARs

`java.fatJar { name; dependencies; mainClass; relocations ? [];
relocateStringConstants ? false; }` merges a library's runtime classpath.
For example, `relocations = [ { from = "org.example"; to = "internal.org.example"; } ];`
rewrites bytecode, package resources, the main class and SPI metadata with pinned
ASM. Rules use the longest package prefix, once; string constants are unchanged
unless explicitly opted in. Reflection, native loading and framework-specific
metadata still require application-level tests.

Inputs may use stored or deflated ZIP entries. Identical duplicates coalesce;
conflicting classes/resources fail. Service providers merge in declaration order.
License/notice files are retained under `META-INF/jix/licenses/<input>/`, with
coordinates in `META-INF/jix/origins.json`. Signatures, old manifests/indexes and
module descriptors are removed. Valid multi-release content is retained and
marked; ambiguous versioned entries fail. Extraction limits are 64 MiB per entry
and 512 MiB total. Output is deterministic and unsigned.

Fat JARs require a classpath-only closure. No minimization, arbitrary transformers,
Gradle Shadow emulation or portable distribution assembly. See the
[fat](templates/fat/build.nix) and [shadow](templates/shadow/build.nix) templates.

### Descriptors outside upstream trees

~~~nix
parser = java.importLibrary {
  descriptor = ./build-support/parser/default.nix;
  src = ./vendor/parser; # Can also be a pinned source input
  # deps = workspace.components.parser; # Opt into root pins
};
~~~

The adapter is a declaration plus a lazy build function:

~~~nix
# build-support/parser/default.nix
{
  spec = { classpaths.main.dependencies = [ ]; };
  lockFile = ./jix.lock;
  build = { java, src, deps, ... }: java.library {
    name = "vendor.parser";
    sources = src + "/src/main/java";
    dependencies = [ deps.classpaths.main ];
  };
}
~~~

By default its own declarations and lock are used. Supplying **deps** selects the
injected view exclusively. Source paths use **src**; descriptor-relative files,
resources, and locks retain ordinary Nix-relative semantics. Nothing is written
into the vendored tree. The same adapter works inside an upstream Jix project.
Maven/Gradle descriptors are not executed or translated automatically; a source
layout alone does not reproduce generated sources or upstream plugin behavior.

To update independent adapter pins, evaluate its **spec** through the dependency
module's **specFile**, then explicitly run:

~~~sh
jix lock /path/to/dependencies.json --output build-support/parser/jix.lock
~~~

### One root lock, optional per component

~~~nix
workspace = inputs.jix.workspaceDependencies {
  inherit pkgs;
  lockFile = ./jix.lock;
  specs = {
    app = import ./maven.nix;
    parser = (import ./build-support/parser/default.nix).spec;
  };
  # repositories = [ "https://repo.maven.apache.org/maven2" ];
};
~~~

Use the result as **deps** in **jix.project** and inject
**workspace.components.parser** into the adapter. Root classpaths become
**app.main**, **parser.main**, etc.; injected views keep local names such as **main**.
Root locking never evaluates Java builds. Component names use letters, digits,
underscores, and hyphens. Repository lists must match exactly, including order,
unless explicitly overridden at the root. Components retaining independent pins
are omitted from **specs**.

### Optional JPMS

Use **moduleDependencies** instead of **dependencies** for module-path inputs, and
**runtimeModuleDependencies** for additional runtime module-path inputs. A library's
own JAR follows the consumer's selected placement; its transitive inputs retain
their declared placement. The same artifact cannot appear on both paths.

Each modular library supplies one module-info.java and uses release 9 or newer.
Jix does not generate requires or exports; the JDK enforces those declarations.
Build components and Java modules are not synonyms.

~~~nix
server = java.library {
  name = "com.example.server";
  sources = ./applications/server/src; # Includes module-info.java
  moduleDependencies = [ core ];
};
app = java.application {
  name = "server";
  moduleDependencies = [ server ];
  module = "com.example.server/server.Main";
};
~~~

Module paths use stable JAR filenames, never Nix store basenames. Maven filenames
come from coordinates; library filenames default to **name + ".jar"** and can be
set with **jarName**. Filename collisions fail rather than renaming automatic
modules. Placing a non-modular JAR on the module path is an explicit choice.
Ordinary classpath entrypoints with module dependencies root those modules using
ALL-MODULE-PATH; named entrypoints use JPMS resolution.

The low-level compile operation accepts **modulepaths** alongside **classpaths**
(entries files). **jix internal modulepath CONFIG --output DIR** creates a stable
jars directory plus entries.json. Legacy Maven entry JSON remains supported;
source-library entries add **kind = "library"**, **jarName**, a stable matching
**id**/**key**, and an empty **version**.

See [the monorepo example](examples/monorepo) for named modules, a vendored
automatic-module adapter, and a classpath app with runtime-only dependencies.
No nested flakes are required.

## Implementation and verification

`jix-nix` uses upstream `nix-bindings-rust`, pinned to
`23de26e2eeb9df5cf26edb6cd17e1fec5fded863`, with Nix 2.34.8 from the pinned
nixpkgs. Small native adapters cover permanent roots and evaluator settings not
exposed by the safe bindings. There is no Nix CLI fallback or alternative evaluator.
Linux x86-64 and AArch64 packages are declared; host validation runs on x86-64.

```sh
nix develop
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
tools/oracle/bootstrap.sh
cargo test --workspace --locked --test differential -- --ignored
nix flake check --print-build-logs
python3 -B tools/test_nix.py
python3 -B tools/test_project.py
python3 -B tools/test_composition.py
```

`python3 -B tools/test_templates.py` exercises scaffolding, packaging and IDE file
ownership. Set `JIX_IDEA_HOME` to an installed IntelliJ home to additionally run
its real JPS compiler, native launches and source-breakpoint smoke tests (tested
with IntelliJ 2025.3.1); this does not modify global IDE settings.

Devenv provides equivalent `check`, `differential`, and `integration` commands.
Native builds require pinned Nix development libraries, bindgen/libclang, and a
C++ compiler; use the provided development shell. The resolver remains Nix-free.
Explicit `jix lock dependencies.json` and `jix resolve dependencies.json` remain
available; raw JSON requires `repositories` and `classpaths`.

All 13 Maven differential suites remain the compatibility gate. See
[COMPATIBILITY.md](COMPATIBILITY.md) for the supported subset and debugging tools.
Lock v5 records context-keyed models and per-classpath ownership for incremental updates.
Versions 1–4 are unsupported; back up the lock and explicitly re-lock to v5. A context edit invalidates only affected classpaths. Serialized `request` / `requestHash` remain legacy
wire names. Old `realize`, `materialize`, and low-level `build` invocations fail
with migration guidance: use `build`, `export`, and `internal` respectively.

Outside this release: Maven/Gradle lifecycle execution, snapshots/ranges, ambient
profiles, private repository auth, annotation processors, module patching, descriptor generation, jlink images,
publishing, automatic Nix-expression editing, and IDE plugins.

### Generated Java sources

Nix owns source generation; Jix consumes its output without IFD. Both
`java.library` and `java.compileJava` accept a single source root or an ordered list:

```nix
java.library {
  name = "application";
  sources = [
    ./src/main/java
    "${generatedJava}/java" # generatedJava is an ordinary Nix derivation
  ];
}
```

Generated directories are inspected only at build time. Existing trees retain
Java-only evaluation filtering, so unrelated resource changes do not recompile
them. Roots must be directories; overlapping/repeated roots, duplicate relative
Java paths and source symlinks fail rather than silently overriding files.
All roots compile together, deterministically, in one javac invocation.

A single `module-info.java` may describe the combined roots. Multiple modules
require separate libraries; classpath builds remain supported. Keep dependency
placement explicit—Jix does not infer modules or generate module descriptors.

`jix idea` realizes and GC-roots generated source outputs without compiling the
selected application. IntelliJ uses immutable generated roots in the store and
editable handwritten roots in the checkout. Rerun it after generator inputs change.

See [the pinned protobuf example](examples/protobuf/README.md) for exact protoc
acquisition, explicit schema inputs, Java generation and the separately locked
protobuf runtime. No plugin system or generic native-artifact resolver is needed.

### Declaration filename

Use `maven.nix` for published Maven dependencies and `build.nix` for sources,
generators, library references and explicit compile/runtime inputs. To migrate an
existing project, rename `dependencies.nix` to `maven.nix` and update its Nix
imports; no Maven relocking is needed. The filename is a convention, not discovery:
existing explicit imports of `dependencies.nix` remain supported.

### Compact lock format and upgrading

Both single and matrix projects write **lock v5**, retaining all model, provenance
and ordering information without duplicate source records or verbose defaults.
Older locks are deliberately unsupported. Upgrade the executable and pinned Jix
input, back up `jix.lock` outside the checkout, then run `jix lock` with repository
access. Compare selections/hashes, verify, build and test before adopting it.
See [the format and complete upgrade instructions](docs/lock-format.md).

## License and releases

Jix is licensed under GPL-3.0-or-later; see [LICENSE](LICENSE).
Third-party fixtures and bundled components retain their own licenses.
See [CONTRIBUTING.md](CONTRIBUTING.md) for validation and maintainer-driven local releases.
