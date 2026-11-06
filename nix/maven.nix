# Pure declarations: only jix lock executes Maven resolution.
{
  resolve =
    {
      dependencies,
      scope,
      platforms ? [ ],
      resolutionContext ? null,
    }:
    if
      !builtins.elem scope [
        "compile"
        "runtime"
        "test"
      ]
    then
      throw "jix: maven.resolve scope must be compile, runtime or test"
    else if !(builtins.isList dependencies && builtins.isList platforms) then
      throw "jix: maven.resolve dependencies and platforms must be lists"
    else
      {
        inherit dependencies scope platforms;
      }
      // (if resolutionContext == null then { } else { inherit resolutionContext; });
}
