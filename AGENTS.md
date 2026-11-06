# Jix

- Read SPEC.md before changing project scope or public interfaces.
- See README.md for workflows and crates/jix/docs/migration.md for the bundled
  `jix migrate --guide` used by migration agents.
- Environment defaults select frozen graphs only; never let host/shell preferences
  alter Maven contexts, dependency hashes, or whole-matrix locking/verification.

- The resolver compatibility gate comes before production locking and Nix Java builds.
- Compare behavior against Maven 3.9.11 / Resolver 1.9.24, not a newer host Maven.
- Preserve classpath and dependency declaration order in every comparison.
- Add a regression fixture for each resolution change; run the Java differential suite.
- Never silently approximate unsupported Maven semantics. Update COMPATIBILITY.md.
- Use Conventional Commits and maintain user-facing CHANGELOG.md entries under Unreleased.
  scripts/release owns version bumps, dated changelog sections, tags and publication;
  do not bump versions or edit published changelog sections for ordinary changes.

- Keep dependency integration independent of the optional Java build DSL.
- Prefer pure `maven.resolve` declarations with required scope. Build inputs consume
  concrete classpaths; do not hide phase selection or edit library metadata.
- Explicit compile/runtime inputs are complete and artifact-only for local libraries;
  never silently inherit compile inputs into runtime. Preserve the legacy API.
- Generators and native tools belong to Nix; generated Java roots must remain
  build-time inputs with IFD disabled. JPMS is supported, not required.
- Test generated-source changes with tools/test_generated_sources.py, including
  IntelliJ realization without application compilation.
- Use upstream nix-bindings-rust for project operations; offline construction must not call Nix.
- Pin native Nix and bindings together; run tools/test_project.py after backend changes.

- Project Maven declarations belong in maven.nix; artifact construction belongs in
  build.nix. These are conventions, not filename-based discovery.
