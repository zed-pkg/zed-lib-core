# TypeSpec and Protobuf persistence cross-checks

TypeSpec fans the locked persistence shadow into JSON Schema and Protobuf 3 so
two independent standard emitters can disagree visibly with the ORM projections.
It does not replace the authored PostgreSQL DDL, the immutable migration ledger,
or the public DTOs owned by `zed-pkg/zed-interfaces`.

## Authority and data flow

```text
authored PostgreSQL DDL (migration authority)
        |
        +--> disposable PostgreSQL --> SeaORM + Drizzle round-trip
        |
        +--> locked persistence JSON shadow
                    |
                    +--> generated TypeSpec
                              |
                              +--> Draft 2020-12 JSON Schema
                              +--> Protobuf 3
```

`schema/persistence.schema.json` remains an imported shadow whose table, column,
type, nullability, interface revision, SeaORM source blobs, and production SQL
blob are checked elsewhere. `tools/typespec-protobuf-parity.mjs` generates one
TypeSpec model surface from that locked shadow, invokes the pinned official
JSON Schema and Protobuf emitters, then independently parses and checks both
outputs. The TypeSpec file is generated evidence, not another hand-authored
schema.

## Explicit wire transformations

The persistence model and proto3 do not have identical semantics. The generator
therefore records every lossy or encoded field in
`generated/schema-contracts/manifest.json`:

| Persistence shape | TypeSpec / JSON Schema | Protobuf | Rule |
| --- | --- | --- | --- |
| UUID | formatted string | string | UUID syntax remains a JSON constraint; Protobuf consumers validate it separately |
| `timestamptz` | RFC 3339 formatted string | string | avoids inventing precision and timezone conversion through `Timestamp` |
| nullable scalar | optional property | proto3 `optional` | absence is preserved; explicit JSON `null` is not |
| required scalar | required JSON property | ordinary proto3 field | proto3 does not enforce required presence |
| `jsonb` object | base64 `bytes` | `bytes` | contents are opaque and remain governed by the persistence/interface schema |
| `int64` | decimal string | `int64` | prevents JavaScript number truncation in the JSON projection |
| JSON arrays | JSON arrays | `repeated` fields | unset versus empty is not distinguished |

These artifacts are suitable for parity review, descriptor generation, and
cross-runtime test fixtures. They are not a new external API: public wire models
remain in `zed-interfaces`, and generated Protobuf must not be published as a
service contract without a separate interface-ownership review.

## Stable Protobuf identities

`schema/typespec-protobuf.lock.json` is an append-only compatibility ledger for
message fields. Generation fails unless every active field has one valid,
unique number. Numbers in Protobuf's implementation-reserved `19000..19999`
range are rejected.

An explicit lock update:

- keeps existing numbers even if source properties are reordered;
- allocates a new number only for a new field;
- moves a removed field's name and number into `reserved`;
- refuses to reintroduce a reserved field name; and
- retires a removed message so its name cannot silently be reused in the same
  `zed.registry.v1` package.

Changing field numbers by hand, deleting reservations, or reusing a retired
message requires a new protocol package and a separately reviewed compatibility
plan.

## Commands

```bash
npm ci --prefix schema-tooling --ignore-scripts --no-audit
node tools/typespec-protobuf-parity.mjs --check
node --test tests/typespec-protobuf-parity.test.mjs
```

For a reviewed schema addition or removal:

```bash
node tools/typespec-protobuf-parity.mjs --write --update-lock
git diff -- schema/typespec-protobuf.lock.json generated/schema-contracts
node tools/typespec-protobuf-parity.mjs --check
```

Ordinary regeneration uses `--write` without `--update-lock`; it must not change
wire identities as a side effect.

## Release gate

The TypeSpec/Protobuf check is tied to the same source commit as the ORM and
schema Zed packages. A green check proves deterministic cross-projection at that
commit. It does not make the generated files package authority, and it does not
replace the read-only declarative-migrations plan against the live PostgreSQL
catalog.
