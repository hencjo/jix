# Published profile regression corpus

Exact, unmodified POM bytes fetched directly from Maven Central, as recorded in
`corpus.json`.
The manifest records each source URL and SHA-256; POMs retain upstream license
and attribution metadata. Refresh explicitly with `tools/capture_profile_corpus.py`.

`selection.json` is Rust's combined ordered compile/runtime selection.
The ignored `published_environment_profiles_parity` test verifies every POM hash,
then compares each root and the combined graph against Maven 3.9.11.
Temporary JARs are synthetic empty fixtures: no upstream binary verification claim.

Context values are explicit test inputs, not detected application host facts.
`java.home` is a synthetic absolute metadata path, not an installed JDK.
This is a dependency-resolution regression fixture, not a complete application
dependency graph or a claim of Gradle selection equivalence.

All 82 POMs were independently fetched from the public source before recording
their URLs and hashes. Seven replaced cached copies with upstream repository or
plugin-repository declarations restored; the other 75 were byte-identical.
Descriptor repositories are ignored by the resolver and isolated Maven oracle;
the ordered selection snapshot remains the compatibility check.
