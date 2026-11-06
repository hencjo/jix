{
  pkgs,
  jix,
  java,
}:
let
  inherit (pkgs) lib;
  fail = message: throw "jix: ${message}";
  empty = {
    classpath = [ ];
    modulepath = [ ];
  };
  key = entry: "${entry.kind or "maven"}:${entry.key}";
  unique =
    entries:
    (lib.foldl'
      (
        state: entry:
        let
          k = key entry;
        in
        if builtins.hasAttr k state.seen then
          if state.seen.${k} == entry then
            state
          else
            fail "conflicting Java inputs for ${k}; reconcile dependency pins explicitly"
        else
          {
            seen = state.seen // {
              "${k}" = entry;
            };
            values = state.values ++ [ entry ];
          }
      )
      {
        seen = { };
        values = [ ];
      }
      entries
    ).values;
  merge =
    a: b:
    let
      cp = unique (a.classpath ++ b.classpath);
      mp = unique (a.modulepath ++ b.modulepath);
    in
    if builtins.any (x: builtins.any (y: key x == key y || x.path == y.path) mp) cp then
      fail "artifact appears on both classpath and module path"
    else
      {
        classpath = cp;
        modulepath = mp;
      };
  collect =
    phase: placement: inputs:
    lib.foldl' (
      result: input:
      let
        metadata = input.jixLibrary or null;
        part =
          if metadata != null then
            if metadata.version != 1 then
              fail "unsupported Java library interface"
            else if phase == null then
              empty // { ${placement} = [ metadata.entry ]; }
            else
              merge (empty // { ${placement} = [ metadata.entry ]; }) metadata.${phase}
          else if input ? jixEntries then
            empty // { ${placement} = input.jixEntries; }
          else
            fail "Java dependency must be a Jix library or locked classpath";
      in
      merge result part
    ) empty inputs;
  closure =
    phase: cp: mp:
    merge (collect phase "classpath" cp) (collect phase "modulepath" mp);
  explicitInputs =
    value:
    if
      !builtins.isAttrs value
      || lib.subtractLists [ "classpath" "modulepath" ] (builtins.attrNames value) != [ ]
    then
      fail "Java inputs require only classpath and modulepath"
    else if
      !(builtins.isList (value.classpath or [ ]) && builtins.isList (value.modulepath or [ ]))
    then
      fail "Java classpath and modulepath inputs must be lists"
    else
      closure null (value.classpath or [ ]) (value.modulepath or [ ]);
  explicitMode = args: args ? compile || args ? runtime;
  mixedMode =
    args:
    explicitMode args
    && builtins.any (name: builtins.hasAttr name args) [
      "dependencies"
      "moduleDependencies"
      "runtimeDependencies"
      "runtimeModuleDependencies"
    ];
  inputLibraries = value: (value.classpath or [ ]) ++ (value.modulepath or [ ]);
  paths =
    name: values:
    let
      make =
        mode: entries:
        let
          config = pkgs.writeText "${name}-${mode}.json" (builtins.toJSON { inherit entries; });
          output = pkgs.runCommand "${name}-${mode}" { } ''
            ${jix}/bin/jix internal ${mode} ${config} --output "$out"
          '';
        in
        output
        // {
          entriesFile = "${output}/entries.json";
          jixEntries = entries;
        };
    in
    {
      classpath = make "classpath" values.classpath;
      modulepath = make "modulepath" values.modulepath;
    };
  validName = name: builtins.match "[A-Za-z0-9_][A-Za-z0-9_.-]*" name != null;
  api = rec {
    library =
      args@{
        name,
        sources,
        dependencies ? [ ],
        moduleDependencies ? [ ],
        runtimeDependencies ? [ ],
        runtimeModuleDependencies ? [ ],
        compile ? { },
        runtime ? { },
        resources ? [ ],
        jdk ? pkgs.jdk21,
        release ? 21,
        manifest ? { },
        javacOptions ? [ ],
        jarName ? "${name}.jar",
      }:
      let
        compileInputs =
          if explicitMode args then
            explicitInputs compile
          else
            closure "compile" dependencies moduleDependencies;
        runtimeInputs =
          if explicitMode args then
            explicitInputs runtime
          else
            merge (closure "runtime" dependencies moduleDependencies) (
              closure "runtime" runtimeDependencies runtimeModuleDependencies
            );
        libraries =
          if explicitMode args then
            inputLibraries compile ++ inputLibraries runtime
          else
            dependencies ++ moduleDependencies ++ runtimeDependencies ++ runtimeModuleDependencies;
        inputs = paths name compileInputs;
        classes = java.compileJava {
          name = "${name}-classes";
          inherit
            sources
            jdk
            release
            javacOptions
            ;
          classpaths = [ inputs.classpath ];
          modulepaths = [ inputs.modulepath ];
        };
        archive = java.jar {
          inherit
            name
            classes
            resources
            manifest
            ;
        };
        entry = {
          kind = "library";
          id = name;
          key = name;
          version = "";
          path = archive.jarFile;
          inherit jarName;
        };
      in
      if mixedMode args then
        fail "cannot mix explicit compile/runtime inputs with legacy dependency arguments"
      else if !validName name || !validName jarName || !lib.hasSuffix ".jar" jarName then
        fail "library requires a stable name and .jar filename"
      else
        archive
        // {
          inherit classes;
          jixLibrary = {
            version = 1;
            identity = name;
            inherit entry;
            compile = compileInputs;
            runtime = runtimeInputs;
            ide = {
              sources = import ./sources.nix { inherit lib; } sources;
              resources = map (r: {
                from = toString r.from;
                into = r.into or "";
              }) resources;
              jdk = toString jdk;
              inherit release javacOptions;
              libraries = builtins.filter (d: d ? jixLibrary) libraries;
            };
          };
        };
    importLibrary =
      {
        descriptor,
        src,
        deps ? null,
      }:
      let
        recipe = import descriptor;
        selected =
          if deps != null then
            deps
          else if !(recipe ? lockFile) then
            fail "imported library has no lockFile; provide dependency views or lock its specification"
          else
            java.dependencies {
              spec = recipe.spec;
              lockFile = recipe.lockFile;
            };
      in
      builtins.seq (selected.lockValid or (selected.descriptor.lockValid or true)) (
        recipe.build {
          inherit pkgs src;
          java = java // api;
          deps = selected;
        }
      );
    fatJar =
      {
        name,
        dependencies,
        mainClass,
        relocations ? [ ],
        relocateStringConstants ? false,
      }:
      let
        runtime = closure "runtime" dependencies [ ];
        helper = import ../relocator.nix { inherit pkgs; };
        config = pkgs.writeText "${name}-fat.json" (
          builtins.toJSON {
            entries = runtime.classpath;
            inherit relocations relocateStringConstants mainClass;
            relocator = if relocations == [ ] then null else "${helper}/bin/jix-relocate";
          }
        );
        result = pkgs.runCommand "${name}.jar" { } ''
          ${jix}/bin/jix internal fat-jar ${config} --output "$out"
        '';
      in
      if runtime.modulepath != [ ] then
        fail "fatJar requires a classpath closure, not module-path inputs"
      else
        result
        // {
          jarFile = toString result;
          jixApplication = {
            inherit mainClass;
            module = null;
            libraries = builtins.filter (d: d ? jixLibrary) dependencies;
            inherit runtime;
            jdk = toString pkgs.jdk21;
          };
        };
    application =
      args@{
        name,
        runtime ? { },
        dependencies ? [ ],
        moduleDependencies ? [ ],
        mainClass ? null,
        module ? null,
        jdk ? pkgs.jdk21,
      }:
      let
        selectedRuntime =
          if args ? runtime then
            explicitInputs runtime
          else
            closure "runtime" dependencies moduleDependencies;
        libraries = if args ? runtime then inputLibraries runtime else dependencies ++ moduleDependencies;
        inputs = paths "${name}-runtime" selectedRuntime;
        entry =
          if (mainClass == null) == (module == null) then
            fail "application requires exactly one of mainClass or module (module/mainClass)"
          else if module != null && builtins.match "[A-Za-z0-9_.]+/[A-Za-z0-9_.$]+" module == null then
            fail "module entrypoint must be module/mainClass"
          else if module == null then
            [ mainClass ]
          else
            [
              "--module"
              module
            ];
      in
      if mixedMode args then
        fail "cannot mix explicit runtime inputs with legacy dependency arguments"
      else
        (pkgs.writeShellApplication {
          inherit name;
          text = ''
            classpath="$(${pkgs.coreutils}/bin/cat ${inputs.classpath}/classpath)"
            if [ -z "$classpath" ]; then classpath=${inputs.classpath}/jars; fi
            exec ${jdk}/bin/java -cp "$classpath" \
              ${
                lib.optionalString (
                  selectedRuntime.modulepath != [ ] || module != null
                ) "--module-path ${inputs.modulepath}/jars"
              } ${
                lib.optionalString (
                  module == null && selectedRuntime.modulepath != [ ]
                ) "--add-modules ALL-MODULE-PATH"
              } ${lib.escapeShellArgs entry} "$@"
          '';
        })
        // {
          jixApplication = {
            inherit mainClass module;
            runtime = selectedRuntime;
            jdk = toString jdk;
            libraries = builtins.filter (d: d ? jixLibrary) libraries;
          };
        };
  };
in
api
