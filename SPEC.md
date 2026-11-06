# Jix: scope

## Is

- Nix-only project tooling, using upstream nix-bindings-rust.
- A Rust resolver for published Maven dependencies, gated against Maven 3.9.11 /
  Resolver 1.9.24; supported semantics are listed in COMPATIBILITY.md.
- Explicit, ordered dependency declarations; independent reusable classpaths.
- Pure Maven resolution declarations with explicit scope, executed only by locking.
- Complete Java compile/runtime input sets; scope is not reapplied during consumption.
- Declared Maven profile contexts, context-isolated models and compact v5 locks.
- One-time init capture of Java/OS properties into a committed context: pinned build
  JDK for applications, PATH Java for dependency-only projects; never recaptured
  by resolution, verification or builds.
- Named dependency environments, one matrix lock, and frozen variant selection with explicit overrides and project/shell defaults.
- Two separate pins: flake.lock for tooling, jix.lock for Maven artifacts.
- Frozen builds: Nix fetchurl realizes hash-verified artifacts without compilation.
- Portable dependency export and offline verification/construction primitives.
- Dependency integration usable without the optional Java build DSL.
- An optional declarative Java library, resource, JAR and launcher DSL.
- Composable local modules, external Jix libraries and vendored-source adapters
  whose descriptors may live outside upstream trees.
- Explicit classpath/module-path placement; existing Java module descriptors.
- Ordered handwritten/generated Java roots compiled together without IFD.
- Nix-owned generators and build-tool pins, independent of Maven runtime locks.
- Optional JPMS: one explicit module per compilation, never mandatory modularization.
- Thin, fat, explicit package-relocated and multi-module project templates.
- Deterministic fat JARs: ordered inputs, SPI merging, per-input license/notice
  retention, unsigned output, preserved valid multi-release entries.
- Pinned ASM bytecode relocation; reflection-string rewriting is explicit opt-in.
- Owned repository-local .idea configuration for native IntelliJ development;
  dependencies/JDKs in the store, local compiler output under .jix/idea-out.
- A bundled Markdown migration guide requiring no skill or MCP installation.
- Conservative: collisions and unrepresentable behavior fail, not approximate.

## Is not

- A reader or automatic migrator for obsolete lock versions 1–4.

- A Maven/Gradle lifecycle, plugin, task or build-script implementation.
- A repackaging of Maven Central into a manually maintained Nix package set.
- A non-Nix project frontend or an ambient ~/.m2 dependency consumer.
- Host-detected profile activation during locking/builds, arbitrary filesystem probes or ignored-profile allowlists.
- Implicit Maven-context inference, cross-compilation setup or foreign execution.
- An automatic build migration, dependency editor or lock updater during builds.
- Gradle Shadow compatibility, minimization, arbitrary resource transformers,
  signature preservation, or portable application distribution.
- Automatic annotation processing, built-in generator plugins, resource filtering,
  module descriptor generation/patching, or jlink.
- A publisher or a dedicated Java test framework/runner.
- An IDE plugin, global SDK installer, or owner of .idea/workspace.xml/user edits.
- An IntelliJ reproduction of every Nix build: remapped resources, overlapping
  roots, app-only dependencies and unnamed-source explicit module paths fail.
- A promise that native IDE output equals the authoritative Nix artifact.

Details and evidence: README.md, COMPATIBILITY.md.
