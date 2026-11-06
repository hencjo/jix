# Project defaults select frozen environments, never Maven evaluation inputs.
let
  single =
    # One per-system definition for the flake and embedded frontend.
    {
      pkgs,
      deps,
      targets ? { },
      defaultTarget ? "dependencies",
      apps ? { },
      defaultApp ? null,
    }:
    let
      inherit (pkgs) lib;
      fail = message: throw "jix: ${message}";
      validName =
        name: name != "" && name != "." && name != ".." && builtins.match "[A-Za-z0-9_.-]+" name != null;
      reserved = [
        "default"
        "dependencies"
      ]
      ++ map (name: "classpath-${name}") (builtins.attrNames deps.classpaths);
      checkedTargets =
        if
          builtins.any (
            name: !validName name || builtins.elem name reserved || lib.hasPrefix "classpaths." name
          ) (builtins.attrNames targets)
        then
          fail "invalid or reserved project target name"
        else
          targets;
      allTargets = deps.descriptor.targets // checkedTargets;
      selectedDefault =
        if builtins.hasAttr defaultTarget allTargets then
          allTargets.${defaultTarget}
        else
          fail "unknown default target: ${defaultTarget}";
      frozen = value: builtins.seq deps.descriptor.lockValid value;
      packages = {
        dependencies = deps.all;
        default = frozen selectedDefault;
      }
      // lib.mapAttrs' (name: cp: lib.nameValuePair "classpath-${name}" cp) deps.classpaths
      // lib.mapAttrs (_: frozen) checkedTargets;
      checkedApps =
        if builtins.any (name: !validName name || name == "default") (builtins.attrNames apps) then
          fail "invalid or reserved app name"
        else
          lib.mapAttrs (
            name: app:
            if app.type or null != "app" || !(builtins.isString (app.program or null)) then
              fail "invalid app: ${name}"
            else
              app // { program = frozen app.program; }
          ) apps;
      allApps =
        checkedApps
        // lib.optionalAttrs (defaultApp != null) {
          default =
            if builtins.hasAttr defaultApp checkedApps then
              checkedApps.${defaultApp}
            else
              fail "unknown default app: ${defaultApp}";
        };
    in
    {
      inherit packages;
      apps = allApps;
      descriptor = deps.descriptor // {
        inherit defaultTarget;
        targets = allTargets;
        apps = allApps;
        ide = import ./ide-model.nix {
          inherit lib;
          targets = checkedTargets;
        };
      };
    };
in
args@{
  pkgs,
  deps,
  forEnvironment ? null,
  defaultEnvironment ? null,
  ...
}:
if deps ? environments then
  let
    inherit (pkgs) lib;
    unexpected = lib.subtractLists [ "pkgs" "deps" "forEnvironment" "defaultEnvironment" ] (
      builtins.attrNames args
    );
    projects = lib.mapAttrs (
      name: selected:
      let
        config =
          if forEnvironment == null then
            { }
          else
            forEnvironment {
              inherit name;
              deps = selected;
            };
        checked =
          if config ? pkgs || config ? deps then
            throw "jix: forEnvironment cannot replace pkgs or selected deps"
          else
            config;
      in
      single (
        {
          inherit pkgs;
          deps = selected;
        }
        // checked
      )
    ) deps.environments;
    selectedDefault =
      if defaultEnvironment == null then
        null
      else if builtins.isString defaultEnvironment && builtins.hasAttr defaultEnvironment projects then
        projects.${defaultEnvironment}
      else
        throw "jix: invalid defaultEnvironment; expected one of: ${lib.concatStringsSep ", " (builtins.attrNames projects)}";
    qualify =
      field:
      lib.foldl' (
        acc: name:
        acc
        // lib.mapAttrs' (
          target: value: lib.nameValuePair "${name}--${target}" value
        ) projects.${name}.${field}
      ) { } (builtins.attrNames projects);
  in
  if unexpected != [ ] then
    throw "jix: matrix project targets/apps must be declared through forEnvironment"
  else
    {
      packages =
        qualify "packages"
        // lib.optionalAttrs (selectedDefault != null) {
          default = selectedDefault.packages.default;
        };
      apps =
        qualify "apps"
        // lib.optionalAttrs (selectedDefault != null && selectedDefault.apps ? default) {
          default = selectedDefault.apps.default;
        };
      descriptor = deps.descriptor // {
        inherit defaultEnvironment;
        environments = lib.mapAttrs (_: project: project.descriptor) projects;
      };
    }
else if defaultEnvironment != null then
  throw "jix: defaultEnvironment requires matrix dependencies"
else if forEnvironment != null then
  throw "jix: forEnvironment requires matrix dependencies"
else
  single (
    builtins.removeAttrs args [
      "forEnvironment"
      "defaultEnvironment"
    ]
  )
