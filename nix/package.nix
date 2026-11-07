# Shared by the flake and downstream packagers; no flake evaluation required.
{
  lib,
  rustPlatform,
  makeWrapper,
  pkg-config,
  nix,
  boost,
  boehmgc,
  curl,
  jdk21,
  bash,
  python3,
  testers,
  src,
  version,
  oracle,
  jixSource ? src,
}:

assert lib.assertMsg
  (nix.version == lib.removeSuffix "\n" (builtins.readFile ../crates/jix-nix/nix-version))
  "Jix requires the native Nix version in crates/jix-nix/nix-version; update and test the native library and Rust bindings together";
rustPlatform.buildRustPackage (finalAttrs: {
  pname = "jix";
  inherit version src;
  # Retain the complete source as a runtime input: init must not depend on a
  # compiler checkout, a moving GitHub branch, or fetching a flake at build time.
  JIX_FLAKE_SOURCE = "path:${jixSource}";

  cargoLock = {
    lockFile = ../Cargo.lock;
    # All nix-bindings-* crates share this source/revision. importCargoLock
    # deduplicates Git sources, so one output hash covers the entire repository.
    outputHashes."nix-bindings-util-0.2.2" = "sha256-Qyt2R4GEhHcxr5FnSAdYoGTX7IUC8lg4leqDBsBa998=";
  };

  postPatch = ''
    patchShebangs tools/oracle/run.sh
  '';
  nativeBuildInputs = [
    makeWrapper
    pkg-config
    rustPlatform.bindgenHook
  ];
  # crates/jix-nix/nix-version enforces the validated native ABI at build time.
  buildInputs = [
    nix.dev
    boost
    boehmgc
  ];
  postInstall = ''
    install -Dm644 LICENSE "$out/share/licenses/jix/LICENSE"
    wrapProgram "$out/bin/jix" --prefix PATH : ${lib.makeBinPath [ curl ]}
  '';

  cargoTestFlags = [ "--workspace" ];
  nativeCheckInputs = [
    jdk21
    bash
    python3
    curl
  ];
  JIX_ORACLE_HOME = oracle;
  postCheck = ''
    (cd fixtures/published && sha256sum --check SHA256SUMS)
    python3 -B tools/test_differential.py
    cargo test --workspace --locked --offline --test differential -- --ignored
    cargo test -p jix --locked --offline --test matrix matrix_maven_parity -- --ignored
  '';

  doInstallCheck = true;
  installCheckPhase = ''
    runHook preInstallCheck
    test "$("$out/bin/jix" --version)" = "jix ${version}"
    "$out/bin/jix" --help > /dev/null
    "$out/bin/jix" migrate --guide > guide.md
    test -s guide.md
    runHook postInstallCheck
  '';
  passthru.tests.version = testers.testVersion { package = finalAttrs.finalPackage; };

  meta = {
    description = "Nix-native Java dependencies and builds";
    homepage = "https://github.com/hencjo/jix";
    license = lib.licenses.gpl3Plus;
    mainProgram = "jix";
    platforms = [
      "x86_64-linux"
      "aarch64-linux"
    ];
  };
})
