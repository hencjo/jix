{
  description = "Jix — a Rust-first Maven resolver and Nix-native Java build tool";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/c8f90650c15282fa8656a041bfbbd2403997a9a7";

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
      manifest = builtins.fromTOML (builtins.readFile ./Cargo.toml);
    in
    {
      maven = import ./nix/maven.nix;
      lib =
        { pkgs }:
        import ./nix/lib {
          inherit pkgs;
          jix = self.packages.${pkgs.stdenv.hostPlatform.system}.jix-build;
        };
      project = import ./nix/project.nix;
      workspaceDependencies =
        {
          pkgs,
          specs,
          lockFile,
          repositories ? null,
        }:
        (import ./nix/workspace-dependencies.nix {
          inherit pkgs;
          dependencyApi = import ./nix/dependencies.nix {
            inherit pkgs;
            jix = self.packages.${pkgs.stdenv.hostPlatform.system}.jix-build;
          };
        })
          { inherit specs lockFile repositories; };

      dependencies =
        {
          pkgs,
          spec,
          lockFile,
        }:
        (import ./nix/dependencies.nix {
          inherit pkgs;
          jix = self.packages.${pkgs.stdenv.hostPlatform.system}.jix-build;
        }).dependencies
          { inherit spec lockFile; };
      devShells = forAllSystems (
        system:
        let
          pkgs = import nixpkgs { inherit system; };
        in
        {
          default = pkgs.mkShell {
            nativeBuildInputs = [
              pkgs.cargo
              pkgs.rustc
              pkgs.clippy
              pkgs.rustfmt
              pkgs.pkg-config
              pkgs.rustPlatform.bindgenHook
              pkgs.python3
              pkgs.jdk21
              pkgs.curl
              pkgs.git
              pkgs.cargo-release
              pkgs.gh
              pkgs.bash
            ];
            buildInputs = [
              pkgs.nix.dev
              pkgs.boost
              pkgs.boehmgc
            ];
          };
        }
      );
      packages = forAllSystems (
        system:
        let
          pkgs = import nixpkgs { inherit system; };
          oracle = import ./nix/oracle.nix { inherit pkgs; };
          java = self.lib { inherit pkgs; };
          hello = import ./examples/hello {
            inherit pkgs;
            jix = java;
          };
          jix = pkgs.rustPlatform.buildRustPackage {
            pname = "jix";
            version = manifest.workspace.package.version;
            JIX_FLAKE_SOURCE = "path:${self.outPath}";
            src = pkgs.lib.fileset.toSource {
              root = ./.;
              fileset = pkgs.lib.fileset.unions [
                ./Cargo.toml
                ./Cargo.lock
                ./LICENSE
                ./crates
                ./fixtures
                ./tools
                ./templates
              ];
            };
            cargoLock = {
              lockFile = ./Cargo.lock;
              allowBuiltinFetchGit = true;
            };
            postPatch = ''
              patchShebangs tools/oracle/run.sh
            '';
            nativeBuildInputs = [
              pkgs.makeWrapper
              pkgs.pkg-config
              pkgs.rustPlatform.bindgenHook
            ];
            buildInputs = [
              pkgs.nix.dev
              pkgs.boost
              pkgs.boehmgc
            ];
            meta.license = pkgs.lib.licenses.gpl3Plus;
            postInstall = ''
              install -Dm644 LICENSE "$out/share/licenses/jix/LICENSE"
              wrapProgram "$out/bin/jix" --prefix PATH : ${pkgs.lib.makeBinPath [ pkgs.curl ]}
            '';
            cargoTestFlags = [ "--workspace" ];
            nativeCheckInputs = [
              pkgs.jdk21
              pkgs.bash
              pkgs.python3
              pkgs.curl
            ];
            JIX_ORACLE_HOME = oracle;
            postCheck = ''
              (cd fixtures/published && sha256sum --check SHA256SUMS)
              python3 -B tools/test_differential.py
              cargo test --workspace --locked --offline --test differential -- --ignored
              cargo test -p jix --locked --offline --test matrix matrix_maven_parity -- --ignored
            '';
          };
        in
        {
          inherit jix oracle;
          jix-build = jix.overrideAttrs (_: {
            pname = "jix-build";
            JIX_FLAKE_SOURCE = "";
            cargoBuildFlags = [ "--no-default-features" ];
          });
          default = jix;
          hello = hello.launcher;
          hello-jar = hello.app;
          hello-classes = hello.classes;
          hello-dependency-spec = hello.spec;
          dependencies = hello.dependencies.all;
          classpath-main = hello.classpaths.main;
          classpath-runtime = hello.classpaths.runtime;
        }
      );
      apps = forAllSystems (system: {
        default = {
          type = "app";
          program = "${self.packages.${system}.jix}/bin/jix";
        };
        hello = {
          type = "app";
          program = "${self.packages.${system}.hello}/bin/jix-hello";
        };
      });
      checks = forAllSystems (
        system:
        let
          pkgs = import nixpkgs { inherit system; };
        in
        {
          resolver = self.packages.${system}.jix;
        }
        // (import ./nix/migration-tests.nix {
          inherit pkgs;
          jix = self;
        })
        // (import ./nix/packaging-tests.nix {
          inherit pkgs;
          java = self.lib { inherit pkgs; };
        })
        // (import ./nix/explicit-tests.nix {
          inherit pkgs;
          java = self.lib { inherit pkgs; };
        })
        // (import ./nix/composition-tests.nix {
          inherit pkgs;
          java = self.lib { inherit pkgs; };
        })
        // (import ./nix/tests.nix {
          inherit pkgs;
          jix = self.lib { inherit pkgs; };
        })
      );
    };
}
