# Changelog

## [Unreleased]

### Added
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
