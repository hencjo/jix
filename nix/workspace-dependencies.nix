# This layer knows only Maven specifications and locks, never Java build recipes.
{ pkgs, dependencyApi }:
{
  specs,
  lockFile,
  repositories ? null,
}:
let
  inherit (pkgs) lib;
  fail = message: throw "jix: ${message}";
  names = builtins.attrNames specs;
  normalized = lib.mapAttrs (_: spec: (dependencyApi.dependencySpec spec).spec) specs;
  selectedRepositories =
    if repositories != null then
      repositories
    else if names == [ ] then
      (dependencyApi.dependencySpec { classpaths = { }; }).spec.repositories
    else
      normalized.${builtins.head names}.repositories;
  valid =
    if !builtins.all (name: builtins.match "[A-Za-z0-9_-]+" name != null) names then
      fail "workspace component names require letters, digits, '_' or '-'"
    else if
      repositories == null
      && !builtins.all (name: normalized.${name}.repositories == selectedRepositories) names
    then
      fail "workspace repositories differ; supply an explicit ordered repositories list"
    else
      true;
  matrixNames = builtins.filter (name: normalized.${name} ? environments) names;
  matrix = matrixNames != [ ];
  envNames = lib.unique (
    lib.concatMap (name: builtins.attrNames normalized.${name}.environments) names
  );
  matrixSpec = {
    repositories = selectedRepositories;
    environments = lib.genAttrs envNames (
      env:
      let
        participating = builtins.filter (name: normalized.${name}.environments ? ${env}) names;
        first = normalized.${builtins.head participating}.environments.${env};
        agree = builtins.all (
          name: normalized.${name}.environments.${env}.resolutionContext == first.resolutionContext
        ) participating;
      in
      if !agree then
        fail "workspace environment contexts differ: ${env}"
      else
        {
          inherit (first) resolutionContext;
          classpaths = lib.foldl' (
            acc: name:
            acc
            // lib.mapAttrs' (
              cp: value: lib.nameValuePair "${name}.${cp}" value
            ) normalized.${name}.environments.${env}.classpaths
          ) { } participating;
        }
    );
  };
  spec = builtins.seq valid (
    if matrix then
      (
        if builtins.length matrixNames != builtins.length names then
          fail "workspace cannot mix matrix and single-context specifications"
        else
          matrixSpec
      )
    else
      {
        repositories = selectedRepositories;
        classpaths = lib.foldl' (
          result: name:
          result
          // lib.mapAttrs' (cp: value: lib.nameValuePair "${name}.${cp}" value) normalized.${name}.classpaths
        ) { } names;
      }
  );
  deps = dependencyApi.dependencies { inherit spec lockFile; };
in
if matrix then
  deps
  // {
    environments = lib.mapAttrs (
      env: selected:
      selected
      // {
        components = lib.mapAttrs (name: component: {
          lockValid = deps.descriptor.lockValid;
          classpaths = lib.mapAttrs (
            cp: _: selected.classpaths."${name}.${cp}"
          ) component.environments.${env}.classpaths;
        }) (lib.filterAttrs (_: component: component.environments ? ${env}) normalized);
      }
    ) deps.environments;
  }
else
  deps
  // {
    components = lib.mapAttrs (name: component: {
      lockValid = deps.descriptor.lockValid;
      classpaths = lib.mapAttrs (cp: _: deps.classpaths."${name}.${cp}") component.classpaths;
    }) normalized;
  }
