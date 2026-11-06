# Development and local releases

Use `devenv shell` for the pinned compiler, native Nix libraries, cargo-release,
Python and GitHub CLI. Run `bash scripts/check-release` for the release gate:
Nix checks (including the pinned Maven oracle), Rust formatting/Clippy, release
script tests, Rust/Nix codec parity, and all host integration suites. The optional
real IntelliJ compiler/debugger smoke tests additionally need `JIX_IDEA_HOME`.

## Changelog and versions

Use Conventional Commits. Maintain user-facing entries under `## [Unreleased]`
in `CHANGELOG.md`; do not manually bump Cargo versions or edit released sections.
The local release script prepends a UTC-dated release and a fresh Unreleased
section, leaving historical text intact. Highest populated category wins:

| Category | Bump |
| --- | --- |
| Breaking changes | Minor on 0.x; major on 1.x and later |
| Added, Changed | Minor |
| Fixed, Security, Documentation | Patch |
| Upgrade notes | No bump on its own |

Empty headings and HTML comments are placeholders. Unknown/duplicate categories,
empty releases and malformed comments/fences fail. Optional `{{version}}` and
`{{date}}` substitutions apply only to the section being released. Stable SemVer
only; prerelease publication is not implemented. An explicit version must be at
least the inferred bump. The first release is also a bump from the current Cargo
version; there is no implicit special case for an untagged repository.

## Release

Commit all changes, including new files, first. Configure `origin` to the intended
GitHub repository, push the branch and set its upstream. Releases must run from
origin's default branch (not a hard-coded `main`). Authenticate with `gh auth login`.
The local branch and the remote branch must point at the same commit, and the new
tag must not already exist locally or remotely.

```sh
devenv shell
scripts/release                  # read-only, offline preview
scripts/release 0.3.0            # preview an explicit version
scripts/release --execute       # perform the release
```

The script uses isolated `release.toml` configuration and separate cargo-release
version/hook/commit/tag steps, without crates.io publication or release PRs. It
creates an annotated `jix-vVERSION` tag, validates and packages that exact clean
commit, then atomically pushes only the release commit and tag to origin. It
creates or resumes a GitHub draft, uploads the archive and SHA-256 checksum, and
publishes only after both uploads succeed. API/auth errors stop the release; they
are not treated as a missing GitHub release. No CI job publishes releases.

### Artifacts

`dist/jix-vVERSION/SYSTEM/jix-VERSION-SYSTEM.tar.gz` contains:

- The native Nix runtime closure (`closure.nar`), **not a standalone executable**.
- `source.tar.gz` from the exact Git tag, including lockfiles and build recipes.
- License, README, compatibility contract and changelog.
- Installation instructions and commit/system/store-path provenance.

A sibling `.sha256` verifies the archive. Packages are built and tested for the
**current host only**, either x86_64-linux or aarch64-linux. No cross-architecture
validation is implied. A published release is immutable to this tooling; the
current workflow publishes one host package, not a coordinated multi-host release.
Nix must already be installed on the recipient system. Import requires appropriate
local Nix trust/administrative permissions; do not disable signature verification
for arbitrary downloads. The full closure can be large. Review redistribution
licenses and corresponding-source requirements for bundled third-party software
before distributing it; Jix's source archive alone is not every dependency's source.
Checksums detect corruption, not publisher authenticity.

### Recovery

Failures stop immediately, retaining changes and local commits/tags for inspection.
Nothing is automatically reset. Before tagging, inspect `git diff`/`git status` and
finish or deliberately undo only the release edits; do not blindly bump again.
After tagging, check out the exact tag and retry independently:

```sh
scripts/release --notes 0.2.0
scripts/release --verify-tag jix-v0.2.0
scripts/release --build jix-v0.2.0 --execute
# If the initial atomic push failed, inspect origin and retry that exact push:
git push --atomic origin HEAD:refs/heads/YOUR_RELEASE_BRANCH refs/tags/jix-v0.2.0
scripts/release --upload jix-v0.2.0 \
  --archive dist/jix-v0.2.0/x86_64-linux/jix-0.2.0-x86_64-linux.tar.gz --execute
```

Build/upload helpers preview unless `--execute` is supplied. Upload verifies the
local annotated tag, remote tag object and commit, archive checksum and embedded
commit before contacting release endpoints. Draft assets may be replaced on retry;
already-published releases are refused. Never move a published tag. If the tagged
source needs fixing, make a new release.

## Publication hygiene

Keep credentials outside project trees. `.aws/`, `.codex/`, `.agents/` and `dist/`
are ignored here to prevent accidental Git additions. Ignore rules do not remove
files from Git history, prevent `git add -f`, or make directory snapshots safe.
Jix intentionally includes untracked source files in project snapshots: **do not
put secrets in a project directory and assume `.gitignore` protects them from the
Nix store**. The Nix store is not a secret store.

Before making a repository public:

1. Scan all local refs/history with a secret scanner, with redacted output.
2. Review fixtures, URLs, example configuration and commit metadata for internal
   information a scanner cannot identify.
3. Confirm ownership/permission to publish and audit third-party licensing.
4. Inspect the actual tagged source archive and release assets.

If a real credential is discovered, revoke/rotate it first. Removing the current
file or adding an ignore rule does not revoke it or erase old commits. Coordinate
any necessary history rewrite before publication. A clean scan is evidence, not a
proof that no sensitive information exists.
