{ lib, targets }:
root:
let
  fail = message: throw "jix: ${message}";
  roots = builtins.filter (d: d ? jixLibrary) (builtins.attrValues targets);
  applications = lib.filterAttrs (_: d: d ? jixApplication) targets;
  appRoots = lib.concatMap (d: d.jixApplication.libraries) (builtins.attrValues applications);
  discovered = builtins.genericClosure {
    startSet = map (value: {
      key = value.drvPath;
      inherit value;
    }) (roots ++ appRoots);
    operator =
      item:
      map (value: {
        key = value.drvPath;
        inherit value;
      }) (item.value.jixLibrary.ide.libraries or [ ]);
  };
  all =
    (lib.foldl'
      (
        state: item:
        let
          identity = item.value.jixLibrary.identity;
        in
        if builtins.hasAttr identity state.seen && state.seen.${identity} != item.key then
          fail "conflicting IDE source libraries named ${identity}"
        else
          {
            seen = state.seen // {
              ${identity} = item.key;
            };
            values = state.values ++ [ (item // { key = identity; }) ];
          }
      )
      {
        seen = { };
        values = [ ];
      }
      discovered
    ).values;
  sourceRoots =
    meta:
    if builtins.isList meta.sources then
      meta.sources
    else
      [
        {
          path = meta.sources;
          generated = false;
        }
      ];
  selectedIds = map (d: d.jixLibrary.identity) roots;
  locals = builtins.filter (
    item:
    let
      sources = sourceRoots item.value.jixLibrary.ide;
    in
    builtins.any (
      source: !source.generated && (source.path == root || lib.hasPrefix "${root}/" source.path)
    ) sources
    || (
      builtins.elem item.key selectedIds
      && sources != [ ]
      && builtins.all (source: source.generated) sources
    )
  ) all;
  localIds = map (item: item.key) locals;
  entry =
    e:
    if (e.kind or "maven") == "library" && builtins.elem e.id localIds then
      { module = e.id; }
    else
      {
        path = e.path;
        id = e.id;
        jarName =
          e.jarName or (
            let
              parts = lib.splitString ":" e.id;
            in
            "${builtins.elemAt parts 1}-${builtins.elemAt parts 4}${
              lib.optionalString (builtins.elemAt parts 3 != "") "-${builtins.elemAt parts 3}"
            }.${builtins.elemAt parts 2}"
          );
      };
  paths = c: {
    classpath = map entry c.classpath;
    modulepath = map entry c.modulepath;
  };
  strip = builtins.unsafeDiscardStringContext;
in
{
  version = 2;
  sourceRoot = root;
  modules = map (
    item:
    let
      m = item.value.jixLibrary;
      meta = m.ide;
    in
    {
      name = item.key;
      sources = map (source: {
        path = if source.generated then source.path else strip source.path;
        inherit (source) generated;
      }) (sourceRoots meta);
      resources = map (r: {
        from = strip r.from;
        inherit (r) into;
      }) meta.resources;
      inherit (meta) jdk release javacOptions;
      compile = paths m.compile;
      runtime = paths m.runtime;
    }
  ) locals;
  applications = lib.mapAttrsToList (
    name: value:
    let
      app = value.jixApplication;
    in
    {
      inherit name;
      inherit (app) mainClass module jdk;
      runtime = paths app.runtime;
    }
  ) applications;
}
