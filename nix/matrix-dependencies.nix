# Named environments are dependency variants, never host platform detection.
{
  pkgs,
  codec,
  jix,
  normalizeContext,
  normalizeSingle,
  validateSingle,
  makeSingle,
}:
let
  inherit (pkgs) lib;
  fail = message: throw "jix: ${message}";
  hash = value: builtins.hashString "sha256" (builtins.toJSON value);
  keys =
    label: allowed: value:
    if lib.subtractLists allowed (builtins.attrNames value) == [ ] then
      value
    else
      fail "unknown ${label} fields";
  validName =
    name: name != "" && builtins.match "[A-Za-z0-9_-]+" name != null && !(lib.hasInfix "--" name);
  bareClasspaths =
    repositories: classpaths:
    let
      normalized = (normalizeSingle { inherit repositories classpaths; }).classpaths;
    in
    if builtins.any (cp: (cp.resolutionContext or null) != null) (builtins.attrValues normalized) then
      fail "matrix classpaths cannot declare resolutionContext"
    else
      normalized;
  normalize =
    value:
    let
      r = keys "matrix" [ "repositories" "environments" "classpaths" ] value;
      repositories = r.repositories or [ "https://repo.maven.apache.org/maven2" ];
      shared = if (r.classpaths or null) != null then bareClasspaths repositories r.classpaths else null;
      environments = lib.mapAttrs (
        name: value:
        let
          env = keys "environment" [ "resolutionContext" "classpaths" ] value;
        in
        if !validName name then
          fail "invalid environment name: ${name}"
        else
          {
            resolutionContext = normalizeContext env.resolutionContext;
            classpaths =
              if (env.classpaths or null) != null then
                bareClasspaths repositories env.classpaths
              else if shared != null then
                shared
              else
                fail "environment ${name} needs classpaths";
          }
      ) r.environments;
    in
    builtins.deepSeq shared (
      if environments == { } then
        fail "matrix needs at least one environment"
      else if repositories == [ ] then
        fail "at least one repository is required"
      else
        { inherit repositories environments; }
    );
  envSpec =
    spec: env:
    normalizeSingle {
      inherit (spec) repositories;
      inherit (env) classpaths resolutionContext;
    };
  flattenMap =
    groups:
    lib.foldl' (
      acc: env: acc // lib.mapAttrs' (cp: v: lib.nameValuePair "${env}--${cp}" v) groups.${env}
    ) { } (builtins.attrNames groups);
  flatSpec = spec: {
    inherit (spec) repositories;
    classpaths = flattenMap (lib.mapAttrs (_: env: (envSpec spec env).classpaths) spec.environments);
  };
  flattenLock = spec: lock: {
    version = 5;
    inherit (lock)
      resolver
      artifacts
      provenance
      contextModels
      ;
    request = flatSpec spec;
    requestHash = hash (flatSpec spec);
    resolution.classpaths = flattenMap (
      lib.mapAttrs (_: result: result.classpaths) lock.resolution.environments
    );
    classpathInputs = flattenMap lock.classpathInputs;
  };
  validate =
    spec: lock:
    if
      (lock.version or null) != 5 || (lock.resolver or "") != "maven-3.9.11/resolver-1.9.24/jix-strict-v2"
    then
      fail "unsupported matrix lock format/resolver; run jix lock"
    else if lock.requestHash != hash spec || lock.requestHash != hash (normalize lock.request) then
      fail "stale lockfile; run jix lock"
    else if
      builtins.attrNames spec.environments != builtins.attrNames lock.resolution.environments
      || builtins.attrNames spec.environments != builtins.attrNames lock.classpathInputs
    then
      fail "matrix environment ownership does not match declarations"
    else
      builtins.seq (validateSingle (flatSpec spec) (flattenLock spec lock)) lock;
  projectLock =
    spec: lock: name:
    let
      env = spec.environments.${name};
      request = envSpec spec env;
      classpathInputs = lock.classpathInputs.${name};
      owned = builtins.attrValues classpathInputs;
      paths = lib.unique (lib.concatMap (i: i.provenance) owned);
      models = lib.unique (lib.concatMap (i: i.models) owned);
      resolution = lock.resolution.environments.${name};
      ids = lib.unique (
        lib.concatMap (cp: map (a: a.id) cp.artifacts) (builtins.attrValues resolution.classpaths)
      );
      contextHash = hash env.resolutionContext;
    in
    {
      version = 5;
      inherit (lock) resolver;
      inherit request resolution classpathInputs;
      requestHash = hash request;
      artifacts = lib.getAttrs ids lock.artifacts;
      provenance = lib.getAttrs paths lock.provenance;
      contextModels = lib.optionalAttrs (models != [ ]) {
        ${contextHash} = lib.getAttrs models lock.contextModels.${contextHash};
      };
    };
  dependencies =
    { spec, lockFile }:
    let
      normalized = normalize spec;
      lock =
        if !builtins.pathExists lockFile then
          fail "missing jix.lock; run jix lock"
        else
          validate normalized (codec.decode (builtins.fromJSON (builtins.readFile lockFile)));
      specFile = (pkgs.writeText "dependencies.json" (builtins.toJSON normalized)) // {
        spec = normalized;
        specHash = hash normalized;
        __jixDependencySpec = true;
      };
      environments = lib.mapAttrs (
        name: env:
        let
          selected = projectLock normalized lock name;
          single = makeSingle {
            spec = envSpec normalized env;
            lockFile = null;
            lockData = selected;
          };
          fetched = lib.mapAttrs (
            path: source:
            pkgs.fetchurl {
              url = source.url;
              sha256 = source.sha256;
              name = builtins.baseNameOf path;
            }
          ) selected.provenance;
          selection = {
            environment = name;
            contextHash = hash env.resolutionContext;
            requestHash = lock.requestHash;
          };
          manifest = pkgs.writeText "jix-artifacts.json" (
            builtins.toJSON {
              version = 2;
              inherit selection;
              artifacts = lib.mapAttrs (_: path: toString path) fetched;
            }
          );
          lockedFile = pkgs.writeText "jix.lock" (builtins.toJSON (codec.encode lock));
          all = builtins.seq lock (
            pkgs.runCommand "jix-dependencies-${name}" { } ''
              mkdir -p "$out/classpaths"
              cp ${lockedFile} "$out/jix.lock"
              cp ${manifest} "$out/artifacts.json"
              ${lib.concatStringsSep "\n" (
                lib.mapAttrsToList (cp: value: ''ln -s ${value} "$out/classpaths/${cp}"'') single.classpaths
              )}
            ''
          );
        in
        single
        // {
          inherit all selection;
          descriptor = single.descriptor // {
            inherit all;
            lockValid = builtins.seq lock true;
            targets = single.descriptor.targets // {
              dependencies = all;
            };
          };
        }
      ) normalized.environments;
    in
    {
      inherit specFile environments;
      descriptor = {
        version = 3;
        spec = normalized;
        lockValid = builtins.seq lock true;
        environments = lib.mapAttrs (_: value: value.descriptor) environments;
      };
    };
in
{
  inherit normalize dependencies;
}
