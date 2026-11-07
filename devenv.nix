{ pkgs, ... }:

{
  packages = [
    pkgs.git
    pkgs.gitleaks
    pkgs.cargo-release
    pkgs.gh
    pkgs.curl
    pkgs.python3
    pkgs.pkg-config
    pkgs.nix.dev
    pkgs.boost
    pkgs.boehmgc
    pkgs.rustPlatform.bindgenHook
  ];

  languages.java = {
    enable = true;
    jdk.package = pkgs.jdk21;
  };
  languages.rust = {
    enable = true;
    channel = "stable";
    components = [
      "rustc"
      "cargo"
      "clippy"
      "rustfmt"
      "rust-analyzer"
    ];
  };

  git-hooks.hooks = {
    gitleaks = {
      enable = true;
      name = "gitleaks";
      entry = "${pkgs.gitleaks}/bin/gitleaks git --staged --redact=100 --no-banner --config .gitleaks.toml";
      pass_filenames = false;
    };
    rustfmt.enable = true;
    nixfmt.enable = true;
  };

  scripts.check.exec = ''
    set -euo pipefail
    cargo fmt --all --check
    cargo clippy --workspace --all-targets --locked -- -D warnings
    cargo test --workspace --locked
    python3 -B tools/test_differential.py
  '';
  scripts.differential.exec = ''
    set -euo pipefail
    tools/oracle/bootstrap.sh
    (cd fixtures/published && sha256sum --check SHA256SUMS)
    cargo test --workspace --locked --test differential -- --ignored
    cargo test -p jix --locked --test matrix matrix_maven_parity -- --ignored
  '';

  scripts.integration.exec = "cargo test -p jix --locked --test lock_codec -- --ignored && python3 -B tools/test_explicit.py && python3 -B tools/test_generated_sources.py && python3 -B tools/test_matrix.py && python3 -B tools/test_resolution_context.py && python3 -B tools/test_nix.py && python3 -B tools/test_project.py && python3 -B tools/test_composition.py && python3 -B tools/test_templates.py && python3 -B tools/test_distribution.py";

  enterTest = ''
    cargo test --workspace --locked
  '';
}
