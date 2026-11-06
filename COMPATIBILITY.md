# Compatibility contract and implementation status

## Reference

- Apache Maven 3.9.11, Model Builder 3.9.11, Resolver 1.9.24.
- Maven distribution SHA-256:
  `4b7195b6a4f5c81af4c0212677a32ee8143643401bc6e1e8412e6b06ea82beac`.
- JDK 21, pinned by the Nix input lock. Plain-shell bootstrap requires JDK 21;
  use the Nix package for the exact JDK package used in CI.
- Maven's Plexus container, `ProjectBuilder`, and
  `DefaultRepositorySystemSessionFactory` configure the oracle. No guessed bare
  Resolver session, plugin lifecycle, user settings, or host local repository.
- Synthetic roots declare each requested dependency at compile scope. Classpath
  scope (`compile`, `runtime`, `test`) is a final Maven classpath filter, not a
  declaration of every dependency at that scope.
- Requests specify repository order. Transitive descriptor repositories are ignored;
  undeclared URLs are mirrored to an empty local directory, never the network.

## Current evidence

The checked-in differential suite compares:

- Ordered selected artifact identities, scopes, pruned graph edges, and root
  effective dependency declarations (including classifiers and exclusions).
- Dependency-relevant effective models, including inherited/imported management.
- 24 generated version-conflict DAGs, 32 generated scoped/optional DAGs, and
  16 generated effective-model cases.
- Focused inheritance, interpolation, BOM precedence, managed exclusions, optional
  dependencies, profile, classifier, relocation, path exclusion and cycle fixtures.
- Frozen published JUnit 5.11.4, Reactive Streams 1.0.4, and javax.inject 1
  metadata, verified by SHA-256. Binaries in these
  tests are generated placeholders, never misrepresented as upstream artifacts.

A scope-widening counterexample was reduced to two roots, a shared dependency,
and its child; `scope_conflict_parity` retains that regression.

## Explicit restrictions

- Exact releases only. Snapshots, version ranges, `LATEST`, `RELEASE`, system paths,
  non-4.0.0 POM models and unresolved/ambient properties fail explicitly.
  Ranges in unselected descriptor dependencies are retained as metadata, not resolved.
  Optional transitive system dependencies are filtered, with explicit absolute
  systemPath validation; selected system dependencies remain unsupported.
  Environment-activated profiles require a declared resolutionContext.
- Duplicate direct dependencies fail rather than silently choosing a version.
- Coordinate components are restricted to safe ASCII letters, digits, `_`, `-`,
  and nonempty dot-separated segments. No path traversal or URL encoding.
- Limits: 4 MiB per POM, 128 levels of parent/model/graph/relocation nesting,
  10,000 graph nodes, bounded property expansion. Limit failures are not parity claims.
- The CLI supports public HTTPS, absolute file repositories, and loopback HTTP.
  URL credentials and private authentication are unsupported. Only HTTP 404 permits
  repository fallback; downloads are bounded to 256 MiB. The crate has no Nix
  dependency and delegates repository access to the caller.

## V1 compatibility gate: passing (corpus-bound)

The pinned Java oracle and Rust agree on all 11 differential suites, including
72 generated graph/model cases, sparse managed optional values, profile ordering,
relocation contexts, repository origins, strict missing/malformed-POM failures,
and published JUnit, Reactive Streams and javax.inject metadata. The profile-order
counterexample was reduced with the minimizer and retained as a regression test.

The oracle uses Maven's strict artifact-descriptor policy and checks collection
and artifact resolution errors explicitly. Missing and invalid POMs fail in both
engines; we deliberately do not adopt Maven CLI's permissive incomplete-descriptor
fallback. Duplicate or empty dependency declarations are rejected by Jix rather
than assigned an approximate meaning. Type aliases that duplicate an artifact
identity are also rejected.

This is the tested v1 contract, not proof of equivalence on every possible POM.
New counterexamples require regression fixtures and fixes before expanding the
supported domain. Production locking and the Nix build API build on this
gate; ordinary builds never rerun dependency resolution.

## Implemented build pipeline

1. `jix lock DEPENDENCIES.json --output jix.lock`: explicit network operation with
   ordered public repositories, deterministic schema, request fingerprint,
   compatibility identity, source URLs/hashes, model provenance and named graphs.
2. `dependencySpec` and the standalone Nix `dependencies` module: Nix evaluates declarations, never Maven
   metadata. Validate request/lock agreement. Fetch artifacts separately and share
   identical downloads. Each named classpath is an independent derivation exposing
   ordered `classpath` and deterministic `jars/` symlinks via `classpathFile`.
3. `compileJava`: explicit filtered sources, pinned JDK/release and an ordered list
   of classpath derivations. Deduplicate identical artifacts; reject conflicting
   versions when composing classpaths. Disable implicit annotation processing.
4. `jar`: a separate derivation assembling classes, source/destination resource
   mappings and manifest attributes. Reject collisions/traversal; fix timestamps
   and entry ordering. Thin JARs only; no embedded Nix classpath in the manifest.
5. Verified offline compilation/execution, byte-identical rebuilds, resource-only
   compilation reuse, independent classpath caching, and local binary-cache
   substitution. Cachix works through standard Nix derivations; no custom CAS.

Verification: `nix flake check` runs Rust/unit tests, all 11 differential suites,
corpus hashing, sandboxed compilation/execution, deterministic JAR comparisons,
and builder safety tests. `tools/test_project.py` verifies embedded evaluation, initialization, roots and exports.
`tools/test_nix.py` verifies derivation invalidation,
independent classpaths, stale-lock rejection, actual fixed-output hash failure,
and restoration into an isolated store from a temporary local binary cache.
`jix verify` validates metadata and optionally materialized artifact bytes offline; Nix verifies artifact bytes when fetching.
The example lock was generated from real upstream binaries, unlike the synthetic
JAR placeholders used in resolver-only tests.

Deferred: Maven/Gradle build execution, private repository auth, annotation
processing, advanced JPMS features, publishing, arbitrary commands, and
an extensible test-runner abstraction.

## Upstream references

- https://maven.apache.org/ref/3.9.11/maven-model-builder/
- https://maven.apache.org/ref/3.9.11/xref/org/apache/maven/internal/aether/DefaultRepositorySystemSessionFactory.html
- Maven Resolver 1.9.24 sources: `NearestVersionSelector`, `JavaScopeSelector`,
  `JavaScopeDeriver`, and `ClassicDependencyManager`, downloaded from
  https://repo.maven.apache.org/maven2/org/apache/maven/resolver/maven-resolver-util/1.9.24/maven-resolver-util-1.9.24-sources.jar

The Rust code is an independently implemented compatibility experiment, not a
vendored copy of Maven. The oracle uses upstream binaries with their original
LICENSE/NOTICE files; frozen POMs retain upstream attribution and license metadata.


### Incremental lock compatibility

Lock v5 tracks provenance and context-keyed effective-model ownership per classpath. Reconciliation
preserves unchanged classpaths, resolves changed/new ones, and prunes unowned
inputs. Repository changes invalidate all classpaths. Shared inputs must agree
exactly; conflicts fail before atomically replacing the lock. Resolution within
one invocation shares a repository snapshot, with reads tracked per classpath.
Versions 1–4 are rejected with re-lock guidance. Context
changes invalidate affected classpaths only; contexts replace rather than deep-merge.
POM bytes and artifacts share provenance, but models never leak between contexts.

The frontend uses `build [TARGET]`, `export`, and lock-only `info`; offline
construction is under `internal`. Descriptor v2 adds named targets and a default
target; descriptor v1 retains dependency/classpath builds.


### Source-built libraries and JPMS

Java-library interface v1 preserves ordered compile/runtime closures through local,
vendored, and external-flake dependencies. Source libraries have their own identity
kind; they are never fabricated as Maven coordinates or added to Maven lockfiles.
Legacy Maven construction entries remain valid. Workspace locking namespaces
classpaths and uses context-aware reconciliation without changing Maven selection.

One named module per library, explicit classpath/module-path placement, stable
automatic-module filenames, and modular launches are supported with release 9+.
Named-module compilation uses only its declared source root and keeps implicit
compilation disabled. Ordinary compilations keep an empty source path. The pinned
JDK checks readability and exports. Descriptor generation, module patching,
access overrides, annotation processors, and jlink remain outside this interface.

Vendored Maven/Gradle source layouts use hand-written Jix adapters. Jix does not
execute upstream build lifecycles, discover their generators, or rewrite vendors.


## Packaging and IntelliJ

Thin launchers, deterministic fat JARs and explicit ASM package relocation are
supported; this is not Gradle Shadow compatibility. SPI files merge, conflicting
entries fail, licenses/notices retain input attribution, signatures/descriptors
are removed, and valid multi-release entries survive. No minimization or generic
resource-transformer API. String rewriting is opt-in and not a proof of reflective
application correctness.

`jix idea` generates owned repository-local native IntelliJ modules and Application
run configurations, not an IDE plugin. SDK registration is explicit. Unsupported
root/resource mappings, application-only dependencies and unnamed explicit
module-path layouts fail. Native development compilation is not Nix packaging.
See SPEC.md and README.md for the exact boundary.

## Declared-profile gate (corpus-bound)

The additional differential suites cover JDK prefixes/negation/ranges, OS fields
and family activation, property presence/value/negation, conjunction, default
suppression, POM/session interpolation precedence, and published relative file
activation. All activators are evaluated, including after a false condition.
The oracle uses unmodified Maven classes and an isolated JVM per context because
Maven caches the path separator statically.

Published descriptor requests merge session user properties over system properties
(MNG-7563); model interpolation still gives POM properties precedence. No CLI
profile-ID overrides, settings profiles or host environment are implicitly added.
Required JDK/OS values must be declared; unix/dos family checks require
`path.separator`. No file-facts API exists. A published descriptor lacks a project
directory: relative paths and `${basedir}` conditions are inactive. Conditions
requiring absolute-path filesystem access fail with POM/profile/path diagnostics.
Java regex OS-version activation is explicitly unsupported, not translated into
Rust regex semantics. Maven's synthetic `packaging` activation/interpolation and
nested activation-property names also fail explicitly: their cross-import model
context semantics are outside this gate. Ordinary JDK/OS/property activation
expressions are interpolated using POM properties before declared session values.

`fixtures/profile-published/corpus.json` records 82 exact POMs independently
fetched from Maven Central, with public source URLs and SHA-256 hashes. All twelve
fixture roots are compared individually and combined, for ordered compile/runtime
selection. JARs used by selection tests are synthetic placeholders, not upstream
binaries. The fixture declares Linux/amd64, Java 21.0.12 and a synthetic absolute
java.home; these are explicit test inputs, not inferred application host facts.

Seven public POMs restore upstream repository or plugin-repository declarations
absent from earlier cached copies; the other 75 are byte-identical. Descriptor
repositories remain ignored by the resolver and isolated oracle. The frozen
ordered selection snapshot checks that these metadata differences do not change
dependency selection under the declared context.

This gate covers the metadata corpus, not an entire application migration.
Complete build-tool selection comparison, source generation, application tests,
module and launcher behavior, and deployment integration remain application-level
acceptance work. Frozen builds never rerun the oracle or resolve dependencies.


## Named environment matrices

Matrix declarations are evaluated into concrete contexts and classpaths before
resolution. V5 matrix locks nest ordered selections and ownership by environment/classpath,
sharing artifact/POM provenance and context-keyed effective models. Reconciliation
reuses unchanged pairs and rejects conflicting shared bytes before atomic publication.
Conversion from a single-context lock re-resolves; no cached model is relabeled.
Single and matrix locks use v5 with an explicit kind; old locks must be re-created.

Matrix context/classpath normalization and fingerprints are checked across Rust and
Nix. The matrix oracle fixture compares distinct architecture and Java-version
profiles against Maven 3.9.11, including compile/runtime selections. Host integration
covers explicit selection, scoped realization/export, native flake/embedded parity,
GC roots, workspace context conflicts, and lock ownership tampering.

Project descriptor v3 contains a complete specification and named v2 environment
views, plus optional `defaultEnvironment` (absent means null). Everyday commands
select via explicit flag, then `JIX_ENVIRONMENT`, then the project default. A project
may map its Nix platform to a declared environment. Defaults are outside dependency
hashes and never evaluated for locking; no lock-format migration is needed.
Lock/resolve/info/verify ignore these defaults. Existing-directory exports can use
the validated manifest selection without Nix. Legacy projects ignore the variable.
Manifest v2 identifies the exported environment, context hash and complete lock hash;
byte verification requires the matching environment. Legacy manifest v1 remains valid
for single-context locks. Full-matrix verification without an artifact map validates
all declarations and ownership; a selected manifest verifies only its environment.

This is dependency selection support, not cross-compilation or foreign execution.
The existing unsupported activation/model semantics are not broadened by naming
an environment. Matrix workspace components cannot silently inherit or override
another component's context; mixed matrix/single-context workspace specs are rejected.

## Generated Java sources and IDE model v2

The optional Java DSL accepts one source root or an ordered list; legacy compiler
JSON with one string remains accepted. Derivation outputs and output subdirectories
retain Nix contexts and are traversed only during construction, not evaluation.
Checked-in roots retain Java-only filtering. One javac invocation compiles all roots,
with deterministic traversal, duplicate/overlap rejection and at most one module
descriptor. Classpath builds remain supported; dependency resolution is unchanged.

IDE model v2 carries source-root records (path and generated flag); the renderer
still accepts v1 single-path models. Generated roots are realized and GC-rooted,
while checkout roots retain editable local mappings. Directly targeted generated-only
libraries are included; external dependency libraries remain binary unless they have
workspace source roots. Generator tools and runtime JARs remain separately pinned.


## Explicit resolution and build inputs

`jix.maven.resolve` is a pure Nix declaration constructor requiring compile/runtime/test
scope. Dependency specifications may be attribute sets or functions receiving
`{ maven; }`. Both normalize identically through dependencies, matrices and
workspaces; no resolver semantics, fingerprints or lock formats change.

Complete `compile`/`runtime` Java input sets are additive APIs. Each listed library
contributes its own JAR only, preserving producer and IDE metadata. Runtime does
not inherit compile inputs. Legacy dependency-list APIs retain their previous
closure propagation; mixing old and new forms is rejected.
