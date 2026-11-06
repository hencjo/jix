# Changelog

## [Unreleased]

### Added
- Local, preview-first releases with version inference, validation, Nix closure packages and GitHub uploads.
- GPL-3.0-or-later licensing.

### Documentation
- Rewrite the README around a runnable example and concise workflows; preserve detailed reference material in the user guide.

### Upgrade notes
- Lock format v5 requires an explicit re-lock of older projects; see docs/lock-format.md.
- Release packages require Nix; they are not standalone static executables.
