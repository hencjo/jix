{ pkgs, jix }:
let
  inherit (pkgs) lib;
  maven = import ./maven.nix;
  codec = import ./lock-codec.nix { inherit lib artifactParts; };
  specValue =
    value:
    let
      evaluated = if builtins.isFunction value then value { inherit maven; } else value;
    in
    if evaluated.__jixDependencySpec or false then evaluated.spec else evaluated;
  fail = message: throw "jix: ${message}";
  checkKeys =
    label: allowed: value:
    let
      extra = lib.subtractLists allowed (builtins.attrNames value);
    in
    if extra == [ ] then value else fail "unknown ${label} fields: ${lib.concatStringsSep ", " extra}";
  normalizeDependency =
    value:
    if builtins.isString value then
      value
    else
      let
        d = checkKeys "dependency" [ "module" "classifier" "extension" "exclude" ] value;
      in
      {
        inherit (d) module;
        classifier = d.classifier or "";
        extension = d.extension or "jar";
        exclude = d.exclude or [ ];
      };
  normalizeContext =
    value:
    let
      c = checkKeys "resolution context" [ "systemProperties" "userProperties" ] value;
      maps = {
        systemProperties = c.systemProperties or { };
        userProperties = c.userProperties or { };
      };
    in
    if
      builtins.all (m: builtins.isAttrs m && builtins.all builtins.isString (builtins.attrValues m)) (
        builtins.attrValues maps
      )
    then
      maps
    else
      fail "resolution context properties must be string maps";
  matrix = import ./matrix-dependencies.nix {
    inherit codec;
    inherit pkgs jix normalizeContext;
    normalizeSingle = normalizeRequest;
    validateSingle = validateLock;
    makeSingle = value: (import ./dependencies.nix { inherit pkgs jix; }).dependencies value;
  };
  normalizeRequest =
    input:
    let
      value = specValue input;
    in
    if value ? environments then
      matrix.normalize value
    else
      let
        r = checkKeys "request" [ "repositories" "classpaths" "resolutionContext" ] value;
        shared =
          if (r.resolutionContext or null) == null then null else normalizeContext r.resolutionContext;
      in
      builtins.deepSeq shared {
        repositories = r.repositories or [ "https://repo.maven.apache.org/maven2" ];
        classpaths = lib.mapAttrs (
          name: value:
          let
            c = checkKeys "classpath" [ "scope" "dependencies" "platforms" "resolutionContext" ] value;
            context = if (c.resolutionContext or null) != null then c.resolutionContext else shared;
          in
          if
            builtins.match "[A-Za-z0-9._-]+" name == null
            || builtins.elem name [
              "."
              ".."
            ]
          then
            fail "invalid classpath name: ${name}"
          else if
            !(builtins.elem (c.scope or "compile") [
              "compile"
              "runtime"
              "test"
            ])
          then
            fail "invalid classpath scope"
          else
            {
              scope = c.scope or "compile";
              platforms = c.platforms or [ ];
              dependencies = map normalizeDependency c.dependencies;
            }
            // lib.optionalAttrs (context != null) {
              resolutionContext = normalizeContext context;
            }
        ) r.classpaths;
      };
  hashRequest = request: builtins.hashString "sha256" (builtins.toJSON request);
  stripSlash = s: if lib.hasSuffix "/" s then stripSlash (lib.removeSuffix "/" s) else s;
  component = s: builtins.match "[A-Za-z0-9_-]+(\\.[A-Za-z0-9_-]+)*" s != null;
  artifactParts =
    id:
    let
      p = lib.splitString ":" id;
    in
    if builtins.length p != 5 then
      fail "invalid artifact identity: ${id}"
    else
      let
        g = builtins.elemAt p 0;
        a = builtins.elemAt p 1;
        e = builtins.elemAt p 2;
        c = builtins.elemAt p 3;
        v = builtins.elemAt p 4;
      in
      if
        !(builtins.all component [
          g
          a
          e
          v
        ])
        || (c != "" && !component c)
        || e != "jar"
      then
        fail "invalid JAR identity: ${id}"
      else
        {
          key = lib.concatStringsSep ":" [
            g
            a
            e
            c
          ];
          version = v;
          path = "${lib.replaceStrings [ "." ] [ "/" ] g}/${a}/${v}/${a}-${v}${
            lib.optionalString (c != "") "-${c}"
          }.${e}";
        };
  run =
    name: entries:
    let
      config = pkgs.writeText "jix-classpath.json" (builtins.toJSON { inherit entries; });
    in
    pkgs.runCommand name { } ''${jix}/bin/jix internal classpath ${config} --output "$out"'';
  readLock =
    spec: file:
    if !builtins.pathExists file then
      fail "missing jix.lock; run jix lock"
    else
      validateLock spec (codec.decode (builtins.fromJSON (builtins.readFile file)));
  validateLock =
    request: lock:
    if
      (lock.version or null) != 5 || (lock.resolver or "") != "maven-3.9.11/resolver-1.9.24/jix-strict-v2"
    then
      fail "unsupported lock format/resolver; move jix.lock aside and run jix lock"
    else if
      (lock.requestHash or "") != hashRequest request
      || lock.requestHash != hashRequest (normalizeRequest lock.request)
    then
      fail "stale lockfile; run jix lock"
    else if builtins.attrNames lock.resolution.classpaths != builtins.attrNames request.classpaths then
      fail "lock classpaths do not match request"
    else if
      !(builtins.all (
        id:
        let
          a = lock.artifacts.${id};
          p = artifactParts id;
          source = a.source;
        in
        if
          a.path != p.path
          || !(builtins.elem source.repository request.repositories)
          || source.url != "${stripSlash source.repository}/${p.path}"
          || builtins.match "[0-9a-f]{64}" source.sha256 == null
          || (lock.provenance.${p.path} or null) != source
        then
          fail "invalid artifact/provenance: ${id}"
        else
          true
      ) (builtins.attrNames lock.artifacts))
    then
      fail "invalid lock"
    else if
      !(builtins.all (
        path:
        let
          source = lock.provenance.${path};
        in
        builtins.match "[A-Za-z0-9_./-]+" path != null
        && builtins.all (part: part != "" && part != "." && part != "..") (lib.splitString "/" path)
        && builtins.elem source.repository request.repositories
        && source.url == "${stripSlash source.repository}/${path}"
        && builtins.match "[0-9a-f]{64}" source.sha256 != null
      ) (builtins.attrNames lock.provenance))
    then
      fail "invalid provenance"
    else if
      !(
        builtins.attrNames (lock.classpathInputs or { }) == builtins.attrNames lock.resolution.classpaths
        && builtins.all (
          name:
          let
            inputs = lock.classpathInputs.${name};
            hash = builtins.hashString "sha256" (
              builtins.toJSON (request.classpaths.${name}.resolutionContext or null)
            );
            models = lock.contextModels.${hash} or { };
          in
          (inputs.contextHash or null) == hash
          && builtins.all (path: builtins.hasAttr path lock.provenance) inputs.provenance
          && builtins.all (
            path: builtins.hasAttr path models && builtins.elem path inputs.provenance
          ) inputs.models
          && builtins.all (
            a:
            builtins.hasAttr a.id lock.artifacts && builtins.elem lock.artifacts.${a.id}.path inputs.provenance
          ) lock.resolution.classpaths.${name}.artifacts
        ) (builtins.attrNames lock.classpathInputs)
        &&
          lib.sort builtins.lessThan (
            lib.unique (lib.concatMap (i: i.provenance) (builtins.attrValues lock.classpathInputs))
          ) == builtins.attrNames lock.provenance
        && (
          let
            owned = builtins.filter (i: i.models != [ ]) (builtins.attrValues lock.classpathInputs);
            hashes = lib.unique (map (i: i.contextHash) owned);
            expected = lib.genAttrs hashes (
              h:
              lib.sort builtins.lessThan (
                lib.unique (lib.concatMap (i: i.models) (builtins.filter (i: i.contextHash == h) owned))
              )
            );
          in
          expected == lib.mapAttrs (_: models: builtins.attrNames models) (lock.contextModels or { })
        )
        &&
          lib.sort builtins.lessThan (
            lib.unique (
              lib.concatMap (cp: map (a: a.id) cp.artifacts) (builtins.attrValues lock.resolution.classpaths)
            )
          ) == builtins.attrNames lock.artifacts
      )
    then
      fail "invalid classpath provenance ownership"
    else
      lock;
in
rec {
  dependencySpec =
    value:
    let
      spec = normalizeRequest value;
    in
    (pkgs.writeText "dependencies.json" (builtins.toJSON spec))
    // {
      inherit spec;
      specHash = hashRequest spec;
      __jixDependencySpec = true;
    };
  classpathsFor =
    {
      spec,
      lockFile,
      lockData ? null,
    }:
    let
      normalized = normalizeRequest spec;
      lock = if lockData == null then readLock normalized lockFile else validateLock normalized lockData;
      fetched = lib.mapAttrs (
        id: artifact:
        pkgs.fetchurl {
          url = artifact.source.url;
          sha256 = artifact.source.sha256;
          name = builtins.baseNameOf artifact.path;
        }
      ) lock.artifacts;
    in
    lib.mapAttrs (
      name: cp:
      let
        entries = map (
          selected:
          if !(builtins.hasAttr selected.id fetched) then
            fail "missing locked artifact: ${selected.id}"
          else
            let
              p = artifactParts selected.id;
            in
            {
              inherit (p) key version;
              id = selected.id;
              path = toString fetched.${selected.id};
            }
        ) cp.artifacts;
        keys = map (e: e.key) entries;
        result =
          if builtins.length keys != builtins.length (lib.unique keys) then
            fail "duplicate identity in classpath ${name}"
          else
            run "jix-classpath-${name}" entries;
      in
      result
      // {
        classpathFile = "${result}/classpath";
        entriesFile = "${result}/entries.json";
        jixEntries = entries;
      }
    ) lock.resolution.classpaths;

  dependencies =
    {
      spec,
      lockFile,
      lockData ? null,
    }:
    let
      evaluated = specValue spec;
    in
    if evaluated ? environments then
      matrix.dependencies {
        spec = evaluated;
        inherit lockFile;
      }
    else
      let
        normalized = normalizeRequest evaluated;
        specFile = dependencySpec normalized;
        lock = if lockData == null then readLock normalized lockFile else validateLock normalized lockData;
        classpaths = classpathsFor {
          spec = normalized;
          inherit lockFile lockData;
        };
        fetched = lib.mapAttrs (
          path: source:
          pkgs.fetchurl {
            url = source.url;
            sha256 = source.sha256;
            name = builtins.baseNameOf path;
          }
        ) lock.provenance;
        manifest = pkgs.writeText "jix-artifacts.json" (
          builtins.toJSON {
            version = 1;
            artifacts = lib.mapAttrs (_: path: "${path}") fetched;
          }
        );
        lockedFile = pkgs.writeText "jix.lock" (builtins.toJSON (codec.encode lock));
        all = builtins.seq lock (
          pkgs.runCommand "jix-dependencies" { } ''
            mkdir -p "$out/classpaths"
            cp ${lockedFile} "$out/jix.lock"
            cp ${manifest} "$out/artifacts.json"
            ${lib.concatStringsSep "\n" (
              lib.mapAttrsToList (name: cp: "ln -s ${cp} \"$out/classpaths/${name}\"") classpaths
            )}
          ''
        );
      in
      {
        inherit specFile all classpaths;
        descriptor = {
          version = 2;
          spec = normalized;
          lockValid = builtins.seq lock true;
          defaultTarget = "dependencies";
          targets = {
            dependencies = all;
          }
          // lib.mapAttrs' (name: cp: lib.nameValuePair "classpaths.${name}" cp) classpaths;
          inherit all classpaths;
        };
      };
  lockedClasspaths =
    { dependencies, lockFile }:
    (dependenciesFor {
      spec = dependencies;
      inherit lockFile;
    }).classpaths;
  dependenciesFor = dependencies;
}
