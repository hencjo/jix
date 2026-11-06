# Protobuf without a build plugin

This example uses protoc **4.33.4** (reports `libprotoc 33.4`) and
`protobuf-java:4.33.4`. The exact Maven-published, statically linked Linux x86-64
compiler is fetched by URL and SHA-256 in `protoc.nix`; the runtime is pinned
separately in `jix.lock`. No Gradle cache or host protoc is used.

From this directory, with Jix installed:

```sh
nix flake lock
jix verify
jix run                 # Hello from protobuf
jix idea
```

The example's Jix input points at this repository (`path:../..`). For a standalone
project, replace it with your shared, revision-pinned Jix input, generate and commit
`flake.lock`, and retain `jix.lock`. This repository currently has no published
remote to substitute automatically. The parent flake pins nixpkgs.

Only x86_64-linux is supported by this example. Additional generator platforms
need their own verified executable/hash; do not substitute a newer protoc.
The build-tool platform selects the executable, not Maven profile activation.

`build.nix` declares the complete schema/import directory as a Nix input and runs
protoc in a derivation. Its output subdirectory and the handwritten source tree
feed one javac invocation. Evaluation never reads generated files; IFD remains
disabled. Schema changes regenerate sources automatically. Add any imported
schema trees as explicit inputs and `--proto_path` arguments.

The two roots belong to one JPMS module. JPMS is encouraged, not mandatory: remove
`module-info.java`, use `classpath` instead of `modulepath` in compile/runtime inputs, and replace
the application's `module` with `mainClass = "example.Main"` for a classpath build.

`jix idea` realizes and retains generated sources in the Nix store, marks them
generated in IntelliJ, and keeps handwritten sources in the checkout. Rerun it
after schema changes. The store trees are immutable; never edit generated Java.

Tool acquisition and code generation belong to Nix. Jix only consumes Java roots;
there is no protobuf plugin or native Maven-tool dependency collection.
