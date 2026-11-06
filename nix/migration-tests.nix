{ pkgs, jix }:
let
  inherit (pkgs) lib;
  guide = builtins.readFile ../crates/jix/docs/migration.md;
  parts = builtins.split "~~~nix file=([A-Za-z0-9.-]+)\n" guide;
  files = lib.filter (item: item != null) (
    lib.imap0 (
      index: part:
      if builtins.isList part then
        {
          name = builtins.head part;
          value = builtins.head (lib.splitString "\n~~~" (builtins.elemAt parts (index + 1)));
        }
      else
        null
    ) parts
  );
  examples = builtins.listToAttrs files;
  load = name: import (builtins.toFile name examples.${name});
  application = load "project.nix" {
    inherit pkgs jix;
    src = ../templates/java;
    lockFile = ../fixtures/migration/jix.lock;
  };
  workspace = load "workspace.nix" {
    inherit pkgs jix;
    src = ../examples/monorepo;
    lockFile = ../examples/monorepo/jix.lock;
  };
  system = pkgs.stdenv.hostPlatform.system;
in
{
  migration-guide =
    assert builtins.length files == 2;
    assert
      builtins.attrNames examples == [
        "project.nix"
        "workspace.nix"
      ];
    assert application.packages.default.drvPath == application.descriptor.targets.application.drvPath;
    assert workspace.packages.default.drvPath == workspace.descriptor.targets.parser.drvPath;
    pkgs.runCommand "jix-migration-guide" { } ''
      mkdir empty
      cd empty
      env -u HOME PATH=/nonexistent ${jix.packages.${system}.jix}/bin/jix migrate --guide > ../full.md
      env -u HOME PATH=/nonexistent ${
        jix.packages.${system}.jix-build
      }/bin/jix migrate --guide > ../offline.md
      cmp ../full.md ../offline.md
      test -z "$(ls -A)"
      ${application.apps.default.program} | grep -Fx 'Hello from Jix!'
      test -f ${application.packages.jar}
      test -f ${workspace.packages.parser}
      touch "$out"
    '';
}
