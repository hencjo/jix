# Compact disk representation only; resolution/hash inputs stay canonical.
{ lib, artifactParts }:
let
  stripSlash = s: if lib.hasSuffix "/" s then stripSlash (lib.removeSuffix "/" s) else s;
  fail = message: throw "jix: ${message}";
  keys =
    allowed: value:
    if builtins.isAttrs value && lib.subtractLists allowed (builtins.attrNames value) == [ ] then
      value
    else
      fail "unknown or malformed compact lock fields";
  escape = lib.replaceStrings [ "%" ":" ] [ "%25" "%3A" ];
  unescape =
    value:
    let
      decoded = lib.replaceStrings [ "%3A" "%25" ] [ ":" "%" ] value;
    in
    if escape decoded == value then decoded else fail "invalid module component escape";
  dep =
    expand: value:
    let
      checked = keys [
        "module"
        "type"
        "classifier"
        "scope"
        "optional"
        "optionalValue"
        "exclusions"
      ] value;
      typed =
        builtins.all (k: builtins.isString (checked.${k} or defaults.${k})) [
          "type"
          "classifier"
          "scope"
        ]
        && builtins.isBool (checked.optional or false)
        && ((checked.optionalValue or null) == null || builtins.isBool checked.optionalValue)
        && builtins.isList (checked.exclusions or [ ])
        && builtins.all builtins.isString (checked.exclusions or [ ]);
      parts = map unescape (lib.splitString ":" checked.module);
      defaults = {
        type = "jar";
        classifier = "";
        scope = "";
        optional = false;
        exclusions = [ ];
      };
    in
    if expand then
      if !typed then
        fail "invalid compact dependency field types"
      else if builtins.length parts != 3 then
        fail "expected group:artifact:version in model"
      else
        defaults
        // builtins.removeAttrs checked [ "module" ]
        // {
          group = builtins.elemAt parts 0;
          artifact = builtins.elemAt parts 1;
          version = builtins.elemAt parts 2;
        }
    else
      lib.filterAttrs (k: v: !(builtins.hasAttr k defaults && v == defaults.${k})) (
        builtins.removeAttrs value [
          "group"
          "artifact"
          "version"
        ]
        // {
          module = "${escape value.group}:${escape value.artifact}:${escape value.version}";
        }
      );
  model =
    expand: value:
    let
      v = keys [ "dependencies" "management" "relocation" ] value;
      full = {
        dependencies = map (dep expand) (v.dependencies or [ ]);
        management = map (dep expand) (v.management or [ ]);
        relocation = if (v.relocation or null) == null then null else dep expand v.relocation;
      };
    in
    if expand then full else lib.filterAttrs (_: v: v != [ ] && v != null) full;
  group =
    expand: value:
    let
      v = keys [ "classpaths" ] value;
    in
    {
      classpaths = lib.mapAttrs (
        _: cp:
        let
          c = keys [ "dependencies" "artifacts" "edges" ] cp;
        in
        {
          dependencies = map (dep expand) c.dependencies;
          artifacts = map (keys [
            "id"
            "scope"
          ]) c.artifacts;
          edges = map (keys [
            "from"
            "to"
            "scope"
          ]) c.edges;
        }
      ) v.classpaths;
    };
  transform =
    expand: value:
    let
      matrix = value.request ? environments;
    in
    value
    // {
      contextModels = lib.mapAttrs (_: lib.mapAttrs (_: model expand)) (value.contextModels or { });
      resolution =
        if matrix then
          {
            environments =
              lib.mapAttrs (_: group expand)
                (keys [ "environments" ] value.resolution).environments;
          }
        else
          group expand value.resolution;
    };
  decode =
    input:
    let
      v = keys [
        "version"
        "kind"
        "resolver"
        "request"
        "requestHash"
        "provenance"
        "contextModels"
        "resolution"
        "classpathInputs"
      ] input;
      matrix = v.request ? environments;
      repos = v.request.repositories;
      sources = lib.mapAttrs (
        path: source:
        let
          s = keys [ "repository" "sha256" ] source;
          repo =
            if builtins.isInt s.repository && s.repository >= 0 && s.repository < builtins.length repos then
              builtins.elemAt repos s.repository
            else
              fail "invalid repository reference";
        in
        {
          repository = repo;
          inherit (s) sha256;
          url = "${stripSlash repo}/${path}";
        }
      ) v.provenance;
      ownership = lib.mapAttrs (
        _:
        keys [
          "contextHash"
          "models"
          "provenance"
        ]
      );
      expanded = transform true (
        builtins.removeAttrs v [ "kind" ]
        // {
          classpathInputs =
            if matrix then lib.mapAttrs (_: ownership) v.classpathInputs else ownership v.classpathInputs;
        }
      );
      groups =
        if matrix then builtins.attrValues expanded.resolution.environments else [ expanded.resolution ];
      ids = lib.unique (
        lib.concatMap (
          g: lib.concatMap (cp: map (a: a.id) cp.artifacts) (builtins.attrValues g.classpaths)
        ) groups
      );
      artifacts = lib.genAttrs ids (
        id:
        let
          path = (artifactParts id).path;
        in
        {
          inherit path;
          source = sources.${path} or (fail "missing artifact provenance");
        }
      );
    in
    if (input.version or null) != 5 then
      fail "unsupported lock format; upgrade Jix and its flake input, move jix.lock aside, then run jix lock (repository access required)"
    else if v.kind != (if matrix then "matrix" else "single") then
      fail "lock kind/declarations mismatch"
    else
      builtins.deepSeq expanded (
        expanded
        // {
          provenance = sources;
          inherit artifacts;
        }
      );
  encode =
    value:
    let
      repos = value.request.repositories;
      base = transform false (
        builtins.removeAttrs value [
          "artifacts"
          "models"
        ]
      );
    in
    base
    // {
      kind = if value.request ? environments then "matrix" else "single";
      provenance = lib.mapAttrs (_: source: {
        repository = lib.lists.findFirstIndex (
          r: r == source.repository
        ) (fail "undeclared repository") repos;
        inherit (source) sha256;
      }) value.provenance;
    };
in
{
  inherit decode encode;
}
