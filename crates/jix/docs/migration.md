This guide is bundled with the installed Jix release. It needs no source checkout,
plugin, skill, MCP server, or network access. Printing it performs no migration.
The coding agent following it needs repository access and permission to run tools.
Building requires Nix and its normal runtime; this command does not initialize Nix.

Suggested request: "Read jix migrate --guide, inspect this project, and help migrate
it to Jix. Keep the existing build until verification passes."

## 1. Inspect before editing

Read repository instructions and inspect the working tree. Preserve user changes;
do not stage, commit, remove old build files, or update unrelated locks implicitly.
Treat Maven/Gradle wrappers and plugins as executable project code: inspect before
running, honor the agent's permission boundaries, and never expose credentials.

Inventory each build component, including:
- Maven parent POMs, active profiles, dependency management, BOMs, repositories,
  exclusions, optional dependencies, scopes, and selected transitive versions.
- Gradle settings, subprojects, included builds, version catalogs, platforms,
  constraints, substitutions, variants, configurations, and custom tasks/plugins.
- Java toolchains and release levels; source/resource roots and resource filtering.
- Annotation processors, generated sources/resources, native dependencies, and
  packaging steps such as shaded JARs, manifests, launch scripts, and publication.
- Tests, test fixtures, integration tests, runtime-only dependencies, service-loader
  files, module-info.java, and launcher arguments/environment requirements.
- Existing flakes, Nix build definitions, locks, vendored trees, and external builds.

Record baseline dependency selections in order for each compile/runtime/test use.
Use the existing project's dependency reports and wrappers when authorized;
configuration names and tasks are project-specific, not universal Gradle commands.
Run representative builds, tests, and application checks where possible. Record
commands, tool versions, results, and anything not executed. If no reliable baseline
is available, say so; successful compilation alone cannot prove equivalence.

## 2. Assess the migration

Jix resolves a strict subset of published Maven metadata against Maven 3.9.11 /
Resolver 1.9.24 behavior. It does NOT execute Maven/Gradle lifecycles or reproduce
Gradle's conflict/variant selection. Do not assume matching direct declarations
produce the same transitive graph. Compare actual selected artifacts and ordering.

Supported building blocks include exact-release Maven dependencies, BOM/platform
imports, exclusions, independently reusable classpaths, Java libraries/JARs,
resources, applications, monorepos, external-flake libraries, vendored-source
adapters, and explicit JPMS module paths. Classpath dependency order matters.

Stop and report blockers rather than silently dropping behavior:
- Snapshots, ranges, LATEST/RELEASE, ambient profiles, system dependency paths,
  private-repository authentication, or other unsupported resolver semantics.
- Annotation processing, implicit generators, upstream plugin/task execution,
  resource filtering, arbitrary shading transformers/minimization, publication,
  or other missing build steps.
- Module descriptor generation, module patching, broad access overrides, or jlink.
- Test execution without an explicitly reproduced runner: Jix has no dedicated
  test command. Do not claim that build or verify ran the application's tests.

Do not remove processors/plugins, turn off tests, relax Java access checks, or
rewrite application behavior just to obtain a green build. Propose a scoped adapter
or partial migration and get agreement on any behavior change. Keep ordinary Java
on the classpath; do not introduce JPMS during migration unless requested.

Checkpoint: summarize the component graph, intended lock ownership, proposed Jix
mapping, and unsupported/unverified behavior before broad edits.

## 3. Scaffold without overwriting the project

Jix must be installed; the user does not need its source checkout. Use the installed
release's initializer for its matching flake input and API. For a fresh scaffold:

~~~sh
scratch=$(mktemp -d)
jix init --template java --project "$scratch"
~~~

Inspect the generated files, then integrate only the needed definitions into the
real project. Do not blindly copy starter Java files or overwrite existing flakes.
For an existing flake, init prints a snippet and leaves it untouched; it does not
merge outputs automatically. Without a flake, dependency-only init creates the
minimal files but still refuses collisions. Neither initializer is a Maven importer.

New scaffolds import a generated resolution-context.nix. Init captures Java/OS
properties once from the pinned build JDK for applications, or PATH Java for
dependency-only projects. Review and commit it, including any local java.home
path; that path is metadata, never a tool lookup. Subsequent operations do not
probe the host or refresh the context. Adapt the captured values explicitly to
the migration's intended Maven session, using named contexts for multiple targets.

The packaged binary defaults to its own immutable Jix source snapshot. For sharing,
select an accessible, pinned Jix flake input rather than someone else's local store
path. Use --jix-source with the project's chosen reference; never invent a release
URL. flake.lock pins Nix inputs; jix.lock pins published Maven dependencies.

### Dependency-only adoption

Start with jix init, or integrate the same dependency interface into an existing
flake. Edit maven.nix using real exact versions from the project:

~~~nix
{ maven }:
let
  common = [ "org.example:library:1.2.3" ]; # Placeholder: replace before locking
in {
  classpaths.main = maven.resolve { scope = "compile"; dependencies = common; };
  classpaths.runtime = maven.resolve { scope = "runtime"; dependencies = common; };
  classpaths.test = maven.resolve {
    scope = "test";
    dependencies = common;
    # platforms = [ "group:bom:exact-version" ];
  };
}
~~~

Rich dependency declarations use { module = "group:artifact:version";
exclude = [ "excluded.group:artifact" ]; }, with optional classifier and extension.
A platform can supply an omitted version. The classpath scope filters the result;
it is not a scope applied to each root. Reproduce compile/runtime selections
separately; do not put every Gradle configuration or Maven test dependency into main.

~~~sh
jix lock
jix info --json
jix classpath main
jix export --output ./deps
jix verify
~~~

Use the exported paths explicitly in other tooling. Exporting a Maven-layout
directory does not force Maven/Gradle to adopt Jix's dependency selection.

### Complete library/application definition

Save this as project.nix and call it from the flake's per-system definition with
pkgs, the pinned jix input, src = ./., and lockFile = ./jix.lock. This example uses
empty compile/runtime declarations and example.Main; replace them with the real
project's declarations, entrypoint, source/resource paths, and JDK/release.

~~~nix file=project.nix
{ pkgs, jix, src, lockFile }:
let
  java = jix.lib { inherit pkgs; };
  deps = jix.dependencies {
    inherit pkgs lockFile;
    spec = { maven }: {
      classpaths.main = maven.resolve { scope = "compile"; dependencies = [ ]; };
      classpaths.runtime = maven.resolve { scope = "runtime"; dependencies = [ ]; };
    };
  };
  runtime = { classpath = [ deps.classpaths.runtime ]; };
  jar = java.library {
    name = "application";
    sources = src + "/src/main/java";
    resources = [ { from = src + "/src/main/resources"; } ];
    compile.classpath = [ deps.classpaths.main ];
    inherit runtime;
    jdk = pkgs.jdk21;
    release = 21;
  };
  app = java.application {
    name = "application";
    runtime = runtime // { classpath = [ jar ] ++ runtime.classpath; };
    mainClass = "example.Main";
    jdk = pkgs.jdk21;
  };
in jix.project {
  inherit pkgs deps;
  targets = { inherit jar; application = app; };
  defaultTarget = "application";
  apps.application = { type = "app"; program = "${app}/bin/application"; };
  defaultApp = "application";
}
~~~

For each supported system, expose project.packages as packages.${system},
project.apps as apps.${system}, and project.descriptor as
jix.${system}.dependencies. The initializer's flake demonstrates this wiring.
Keep Maven declarations in maven.nix once integrated instead of maintaining
both an inline specification and a separate copy.

Library outputs are JAR derivations carrying jixLibrary interface v1. Reference the
whole output, not just jarFile: dependencies = [ core ] or
[ inputs.upstream.packages.${system}.core ]. Compile dependencies propagate to
compile/runtime closures; additional runtime dependencies propagate only at
runtime. Preserve first-declaration order. Conflicting identities, versions, or
paths fail instead of silently mediating independently locked libraries. Source
libraries need stable globally distinct names and an acyclic dependency graph;
they are pinned by source control or flake.lock, not added to Maven lockfiles.

### Workspaces and vendored source adapters

Use one root flake with named library targets; a subdirectory needs no nested flake.
Separate a descriptor's location from its source root to keep vendors untouched.
An adapter at build-support/parser/default.nix has this shape:

~~~nix
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

Import it with java.importLibrary { descriptor = ./build-support/parser/default.nix;
src = ./vendor/parser; }. Omit deps to use the adapter's own specification/lock.
The source directory can also be a pinned source input. Descriptor-relative files
and locks retain Nix-relative semantics; sources use explicit src. An upstream Jix
adapter may live inside the upstream tree and uses the same interface.

For root-owned pins, contribute the adapter's specification to the workspace and
inject its dependency view, as in this complete dependency-only project definition:

~~~nix file=workspace.nix
{ pkgs, jix, src, lockFile }:
let
  java = jix.lib { inherit pkgs; };
  descriptor = src + "/build-support/parser/default.nix";
  deps = jix.workspaceDependencies {
    inherit pkgs lockFile;
    specs.parser = (import descriptor).spec;
  };
  parser = java.importLibrary {
    inherit descriptor;
    src = src + "/vendor/parser";
    deps = deps.components.parser;
  };
in jix.project {
  inherit pkgs deps;
  targets = { inherit parser; };
  defaultTarget = "parser";
}
~~~

Root classpaths become parser.main; the injected view retains main. Components
using independent pins stay outside workspace specs. Component names use letters,
digits, underscores, and hyphens. Repository lists must match exactly, including
order, unless the workspace explicitly selects a repositories list. Do not silently
combine repositories or substitute one independently locked version for another.

To create/update independent adapter pins, expose
(jix.dependencies { inherit pkgs; spec = (import descriptor).spec;
lockFile = chosenLock; }).specFile as a temporary flake package, build it, then run
jix lock /nix/store/...-dependencies.json --output build-support/parser/jix.lock.
Evaluating specFile requires no existing Maven lock. Never edit vendored trees or
write a sibling adapter's lock as a side effect of importing/building it.

### Preserve existing JPMS intentionally

Use moduleDependencies for compile/module-path inputs and
runtimeModuleDependencies for additional runtime module-path inputs. One library
may supply one module-info.java, requiring release 9+. Its requires/exports clauses
remain Java source, not generated from the dependency list. A referenced library's
own JAR follows the consumer's selected path; inherited dependencies retain their
placement. The same artifact cannot appear on both paths.

A modular launcher uses java.application { name = "server";
moduleDependencies = [ server ]; module = "com.example.server/server.Main"; }.
Do not also supply mainClass. Module-path JAR filenames are stable: Maven filenames
come from coordinates; source libraries default to name + ".jar" and may set
jarName. Filename collisions fail. Non-modular JARs become automatic modules only
when explicitly put on the module path. Ordinary classpath entrypoints with module
dependencies root those modules with ALL-MODULE-PATH; named entrypoints use JPMS
resolution. Do not use module patching or access overrides to conceal a mismatch.

## 4. Verify the migration

Generate hashes and provenance only through jix lock. Never fabricate lock entries.
Locking preserves unchanged classpaths, resolves changed/new ones, and prunes
unowned inputs. Repository changes invalidate all classpaths. Lock versions 1–4
are unsupported; upgrade the executable and flake input, back up the old lock,
then run jix lock to create v5 with repository access.
Builds/run/export never update either lock. Update Nix inputs only when explicitly
intended, separately from Maven reconciliation.

~~~sh
jix lock
jix info --json
jix verify
jix build
jix run
nix build
nix run
~~~

Select applicable named targets/apps; dependency-only projects have no runnable app.
Compare selected dependency identities, classifiers, versions, scopes, and ordering
against the baseline for each relevant use. Resolve and document differences rather
than assuming Maven or Gradle parity. Check resources, manifests, service-loader
entries, launch behavior, tests, and deployment-relevant artifacts. Byte-identical
JARs are not always the right baseline: compare contents and semantics where build
metadata differs, recording why the difference is acceptable.

jix verify validates lock structure/freshness and optionally artifact hashes; it is
NOT a test runner. Reproduce test execution explicitly before claiming test parity.
Keep the old build available until the agreed checks pass. Do not delete upstream
build files or rewrite vendors merely because Jix compiled successfully.

## 5. Report evidence, not confidence

Finish with:
- Changed files and mappings, including library boundaries and pin ownership.
- Commands actually executed, versions used, and observed results.
- Dependency-selection differences and their disposition.
- Build/run/test/resource checks passed, failed, and not executed.
- Unsupported behavior, remaining blockers, and follow-up actions.

Use "builds successfully" when that is all that was established. Use "migration
verified" only when the agreed baseline comparisons pass; qualify exactly what
was verified. Missing tests, unavailable repositories, or unexecuted generators
remain explicit gaps, not implied successes.


## Templates, packaging and IntelliJ

Use `jix init --template thin|fat|shadow|multi-module --project DIR` in a scratch
directory (choose one template, not the literal pipe-separated text).
`java` remains an alias for thin; the default is dependencies-only.
All application templates retain flake packages/apps. Thin artifacts use a Nix
launcher; fat artifacts embed the runtime classpath. Explicit relocation uses
`java.fatJar { name; dependencies; mainClass; relocations; }`, with rules
`{ from = "upstream.pkg"; to = "internal.upstream.pkg"; }`.
String constants are unchanged unless `relocateStringConstants = true;`.

Do not translate Shadow configuration blindly. Only package/bytecode/resource
relocation and SPI merging are supported; no minimization/custom transformers.
Conflicts fail, licenses/notices retain attribution, output is unsigned, module
descriptors are removed and valid multi-release entries retained. Fat inputs must
be classpath-only. Test reflective and framework-specific behavior explicitly.

After locking, run `jix idea` (optionally `--open`) to generate repository `.idea/`.
Register the printed SDK names/homes in IntelliJ once. Native build/run/debug uses
local sources and `.jix/idea-out/`; verify deployable output separately with Nix.
Jix preserves workspace.xml and refuses unmanaged collisions or modified generated
files; migrate existing Maven/Gradle IDE configuration explicitly. Regenerate after
model changes. Remapped resources, overlapping roots, app-only dependencies and
unnamed explicit module-path layouts are blockers, not silent approximations.


## Published Maven profiles

If locking reports an environment-dependent profile, declare `resolutionContext`
beside `classpaths` in `maven.nix`; do not remove or ignore that profile.
Its `systemProperties` and `userProperties` are string maps. Declare the intended
`java.version`, `os.name`, `os.arch`, `os.version` and `path.separator`; some
metadata also requires an absolute `java.home`. These inputs are not host probes
or Java toolchain selection. A per-classpath context replaces the shared context.
Run `jix lock` explicitly and compare ordered compile/runtime selections with
the original build; Maven profile parity is not Gradle conflict-resolution parity.
Absolute file activation, Java-regex OS activation and Maven's synthetic
`packaging` property fail rather than approximate.
Only v5 locks are supported; old locks must be backed up and re-created explicitly.


## Architecture and Java-version variants

Use named `environments` in `maven.nix` when dependency resolution differs
by platform or Java version. Each environment requires a complete `resolutionContext`;
shared top-level `classpaths` are reused, or an environment supplies its own complete
classpath map. Do not combine this matrix form with global/per-classpath contexts.

`jix lock` locks the entire matrix into one v5 lock. Compare each environment's
ordered selections with the original build, not just the development machine's graph.
Everyday commands (build/classpath/export/run/idea) select via `--environment NAME`,
then `JIX_ENVIRONMENT`, then project `defaultEnvironment`. Missing/invalid selections
fail rather than guessing. Lock/resolve/info/verify ignore defaults; locking always
covers all environments. Existing-directory exports default to the validated
manifest selection, and overrides must match.
Use `jix.project { inherit pkgs deps; forEnvironment = { name, deps }: ...; }`
to reuse a Java recipe. Add `defaultEnvironment = { x86_64-linux = "linux-amd64";
aarch64-linux = "linux-aarch64"; }.${pkgs.stdenv.hostPlatform.system} or null;`
to that project definition for platform-aware selection of already-pinned graphs.
Use your own declared names. Defaults never belong in `maven.nix` and must
not change Maven contexts. Flake outputs retain `ENVIRONMENT--TARGET` and gain default
package/app aliases where configured. Shell overrides affect Jix, not pure flake
outputs; CI should pass `--environment` explicitly.
Workspace components sharing an environment name must agree
on its context. Native classifiers remain explicit declarations, not plugin execution.
This does not configure cross-compilation or make foreign native libraries runnable.

## Generated Java (protobuf and other generators)

Move generator execution into an ordinary Nix derivation; pin the exact tool
separately from Maven runtime dependencies. Do not enable IFD, inspect generator
outputs during evaluation, use a developer's Gradle cache, or silently substitute
the nixpkgs tool version.

Pass handwritten and generated directories together:
`sources = [ ./src/main/java "${generatedJava}/java" ];`.
The existing single-root form still works. Jix compiles all roots together; duplicate
Java paths, overlapping roots, symlinks and multiple module descriptors fail.
Keep generated and handwritten classes in the same existing JPMS module unless
the project deliberately wants a new boundary. Classpath builds remain supported.

For protobuf, pin protoc 4.33.4 independently of `protobuf-java:4.33.4`; they are a
native build executable and a runtime JAR respectively. The repository's
`examples/protobuf` provides a fixed-hash Linux x86-64 protoc recipe and complete
JPMS example. A different build platform requires a separately verified tool pin.

Run `jix idea` after schema/generator changes: it realizes and retains generated
roots, marks them generated, and leaves handwritten sources in the checkout.
It need not compile the application. Never edit the immutable store sources.


## Resolution declarations versus Java inputs

Use `{ maven }: …` specifications and `maven.resolve { dependencies = …; scope = …; }`.
This is a pure declaration, not evaluation-time resolution: only `jix lock` resolves.
Scope is mandatory on the constructor and is never interpreted again by a Java
consumer. Existing attribute-set/JSON specifications remain supported.

Prefer complete `compile = { classpath = …; modulepath = …; };` and `runtime`
input sets. Missing blocks/lists are empty, and runtime does not inherit compile.
A referenced local library supplies only its JAR; explicitly list its needed
dependencies too. Keep reusable input lists in ordinary Nix bindings and add the
application/test JAR explicitly to launcher runtime inputs.

Do not mix explicit blocks with legacy dependency-list arguments. Legacy calls
still propagate transitive closures. Never edit `jixLibrary` metadata to suppress
transitives: explicit inputs provide artifact-only consumption while preserving the
producer's original compilation and IntelliJ model. Compare ordered runtime graphs
when migrating: removing an old compile/runtime union can change ordering even
when the artifact set is identical.

### Breaking lock-format upgrade

Upgrade both the Jix executable and the project's Jix flake input. From the project
root, run:

~~~sh
backup="$(mktemp -d)"
mv jix.lock "$backup/jix.lock"
jix lock
# On failure, restore: mv "$backup/jix.lock" jix.lock
~~~

Versions 1–4 are rejected, never silently migrated. Repository access is required.
Retain the backup: compare ordered selections and artifact hashes using the old
checkout/tooling, because re-resolution can expose changed upstream bytes.
Run jix verify, project builds and tests; regenerate exports and run jix idea.
Commit the new jix.lock and tooling flake.lock together.
