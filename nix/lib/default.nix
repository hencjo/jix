{ pkgs, jix }:
let
  inherit (pkgs) lib;
  fail = message: throw "jix: ${message}";
  checkKeys =
    label: allowed: value:
    let
      extra = lib.subtractLists allowed (builtins.attrNames value);
    in
    if extra == [ ] then value else fail "unknown ${label} fields: ${lib.concatStringsSep ", " extra}";
  run =
    name: mode: config: inputs:
    let
      configFile = pkgs.writeText "jix-build.json" (builtins.toJSON config);
    in
    pkgs.runCommand name {
      nativeBuildInputs = inputs;
    } ''${jix}/bin/jix internal ${mode} ${configFile} --output "$out"'';
  java = (import ../dependencies.nix { inherit pkgs jix; }) // {
    compileJava =
      {
        name ? "java-classes",
        sources,
        classpaths ? [ ],
        modulepaths ? [ ],
        jdk ? pkgs.jdk21,
        release ? 21,
        javacOptions ? [ ],
      }:
      let
        # The resource tree is deliberately absent from this derivation's inputs.
        roots = import ./sources.nix { inherit lib; } sources;
        filtered = lib.imap0 (
          index: root:
          if root.generated then
            root.path
          else
            builtins.path {
              name = "jix-java-sources-${toString index}";
              path = root.path;
              filter =
                path: type:
                if type == "symlink" && lib.hasSuffix ".java" path then
                  fail "Java source symlinks are unsupported"
                else
                  type == "directory" || (type == "regular" && lib.hasSuffix ".java" path);
            }
        ) roots;
      in
      run name "compile" {
        sources = map toString filtered;
        javac = "${jdk}/bin/javac";
        inherit release javacOptions;
        classpaths = map (
          cp: cp.entriesFile or (fail "compileJava requires Jix classpath derivations")
        ) classpaths;
        modulepaths = map (
          mp: mp.entriesFile or (fail "compileJava requires Jix module-path inputs")
        ) modulepaths;
      } [ jdk ];

    jar =
      {
        name ? "application",
        classes,
        resources ? [ ],
        manifest ? { },
      }:
      let
        result = run "${name}.jar" "jar" {
          classes = toString classes;
          inherit manifest;
          resources = map (
            value:
            let
              r = checkKeys "resource" [ "from" "into" ] value;
            in
            {
              from = "${r.from}";
              into = r.into or "";
            }
          ) resources;
        } [ ];
      in
      result // { jarFile = toString result; };
  };
in
java // (import ./libraries.nix { inherit pkgs jix java; })
