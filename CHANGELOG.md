# Changelog

## [Unreleased]

### Added
- Deterministic published Maven dependency resolution, gated against Maven 3.9.11 / Resolver 1.9.24, with explicit contexts and compact v5 locks.
- Pure Nix dependency declarations, frozen classpaths, standard Nix caching, and optional Java/JPMS build helpers.
- Thin, fat, relocated and multi-module application templates, native IntelliJ project generation, and portable dependency exports.
- Explicit first-release support at the current version and separately checksummed source archives.
- Local, preview-first releases with version inference, validation, Nix closure packages and GitHub uploads.
- GPL-3.0-or-later licensing.
- A reusable, flake-independent Nix package recipe with hashed Git dependencies and installed-binary smoke tests.

### Fixed
- Reject duplicate classpath identities and unsupported repository URLs during Nix lock validation, matching Rust verification before derivation construction.
- Give the embedded Nix evaluator a 64 MiB stack so launcher builds do not require `RUST_MIN_STACK`.
- Retry repository download timeouts, refused connections and transient HTTP failures during locking, with bounded exponential backoff.
- Reject unvalidated native Nix library versions rather than silently mixing them with the pinned Rust bindings.

### Documentation
- Clarify which explicit commands work without Nix and distinguish Nix-independent operations from offline project verification.
- Rewrite the README around a runnable example and concise workflows; preserve detailed reference material in the user guide.
- Document profile installation, flake development-shell/build inputs, and downstream nixpkgs packaging requirements.

### Upgrade notes
- Lock format v5 requires an explicit re-lock of older projects; see docs/lock-format.md.
- Release packages require Nix; they are not standalone static executables.
