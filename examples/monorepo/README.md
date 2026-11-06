# Composable Java fixture

- core and server: separate JPMS modules.
- vendor/parser: a tiny Maven-layout **test fixture**, built without Maven.
- build-support/parser: its external adapter and independent empty lock.
- plain and plugin: ordinary Java with a runtime-only dependency.
- unrelated: a separate target for incremental-build checks.

From this directory, with Jix installed:

~~~sh
nix flake lock
jix lock
jix build core
jix run
jix run plain
~~~

The example Jix input points at the enclosing checkout. Replace it with a pinned
published Jix input when copying elsewhere. Keep generated flake.lock; jix.lock
is already included.

build.nix injects the root workspace's parser dependency view. Remove that deps
argument to use the adapter's own lock instead. Neither choice modifies the
vendor directory. The parser.jar filename supplies a stable automatic-module
name without changing its upstream JAR manifest.
