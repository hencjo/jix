{ lib }:
sources:
let
  inputs = if builtins.isList sources then sources else [ sources ];
  roots = map (
    source:
    let
      path = toString source;
      generated =
        lib.isDerivation source
        || builtins.any (context: context ? outputs || context.allOutputs or false) (
          builtins.attrValues (builtins.getContext path)
        );
    in
    if !(builtins.isPath source || builtins.isString source || lib.isDerivation source) then
      throw "jix: sources must contain paths or derivations"
    else
      { inherit path generated; }
  ) inputs;
  overlaps = a: b: a == b || lib.hasPrefix "${a}/" b || lib.hasPrefix "${b}/" a;
  checked = lib.foldl' (
    seen: root:
    if builtins.any (old: overlaps old.path root.path) seen then
      throw "jix: overlapping or repeated source root: ${root.path}"
    else
      seen ++ [ root ]
  ) [ ] roots;
in
checked
