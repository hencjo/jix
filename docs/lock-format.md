# Lock format v5

Only v5 is supported. This is a disk representation, not a change to Maven
resolution, declaration normalization or resolver identity.

## Layout

- `version: 5`, `kind: "single"` or `"matrix"`, and `resolver`.
- `request` and `requestHash`: unchanged normalized declarations and SHA-256 of
  canonical JSON. Repository order is significant.
- `provenance`: repository-relative paths mapped to
  `{"repository":0,"sha256":"…"}`. Repository is a zero-based index into
  `request.repositories`; SHA-256 is 64 lowercase hexadecimal digits.
  The URL is the repository without trailing slashes, then `/`, then the path.
- `contextModels`: context hash → POM path → dependency-relevant effective model.
  An absent map means empty for single locks; matrix locks carry the map.
- `resolution`: ordered dependencies, artifact selections and edges per classpath.
  Matrix locks nest these under `environments`.
- `classpathInputs`: context hash and exact provenance/model ownership per
  classpath, nested by environment in matrix locks.

Artifact IDs retain `group:artifact:extension:classifier:version`. Their
repository-relative paths are derived by the existing Maven layout rules.
Every selected artifact must have provenance; no duplicate artifact source table
is stored. All POM provenance is retained too.

## Compact dependency records

Dependencies in effective models and resolved results use `module: "group:artifact:version"`.
The version component may be empty in a partial effective-model entry. Within
each component, `%` and `:` are escaped as `%25` and `%3A` (uppercase only).
This preserves arbitrary unused model values without ambiguity; normal coordinates
are unchanged. Other percent escape forms are rejected. This is
not the user dependency parser; no defaults are inferred from the host or Maven.

| Omitted field | Exact expanded value |
| --- | --- |
| `type` | `"jar"` |
| `classifier`, `scope` | `""` |
| `optional` | `false` |
| `optionalValue` | absent |
| `exclusions` | `[]` |

Explicit empty `type` is retained. Raw Maven types such as `test-jar` are never
collapsed into their effective extension/classifier. Explicit
`optionalValue: false` is retained. Empty model `dependencies`/`management`
lists and absent `relocation` may be omitted. Other lists remain explicit.
An absent resolution context remains distinct from an explicitly empty context.

The decoder expands this representation before validation and use. Unknown
fields, invalid references and inconsistent ownership fail. Declaration hashes
and environment selection hashes use expanded canonical semantics, never the
pretty-printed bytes. Arrays retain order; object keys are sorted. The writer
uses two-space section indentation with compact leaf records and a final newline.

## Upgrading draft projects

Upgrade both your Jix executable and the pinned Jix flake input first, for example
`nix flake update jix` when the input is named `jix`. Then, from the project root:

```sh
backup="$(mktemp -d)"
mv jix.lock "$backup/jix.lock"
jix lock
# If locking fails:
# mv "$backup/jix.lock" jix.lock
```

Keep the backup outside the checkout. Re-locking requires repository access and
may reveal changed upstream bytes: this is not an offline format conversion.
Use the old checkout/tooling and backup to compare ordered artifact selections
and hashes with the new lock. Then run `jix verify`, project builds and tests.
Regenerate dependency exports and run `jix idea`. Commit `jix.lock` and the updated
`flake.lock` together. Neither builds nor verification rewrite an old lock.

The existing artifact export manifest formats are unchanged, but exports contain
v5 locks and should be regenerated after upgrading.
