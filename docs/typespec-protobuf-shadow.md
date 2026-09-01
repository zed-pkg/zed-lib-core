# TypeSpec, JSON Schema, and Protobuf persistence cross-checks

This document records both the current transition tooling and the target
dual-source contract. In the target, TypeSpec is the authored P0 canonical AST
and an independently authored JSON Schema tree is the P1 secondary-primary
source. P1 is not generated from TypeSpec and has release-veto power. The JSON
Schema emitted from P0 is stored separately as diagnostic evidence and compared
with P1.

The current tool still fans the locked persistence JSON shadow into generated
TypeSpec, JSON Schema, and Protobuf. That direction is useful legacy evidence;
it is not the completed P0/P1 architecture and must not be mislabeled as such.
The immutable DDL/migration lineage remains the deployed P2 baseline until the
authored sources and all parity/migration gates pass. Public DTOs remain owned
by `zed-pkg/zed-interfaces`.

## Authority and data flow

```text
target:
  authored TypeSpec P0 -----> SQL/ORM/wire candidate A
          |                  +-> emitted JSON Schema (diagnostic)
          +------------------+-> Protobuf 3

  independent JSON P1 -----> SQL/ORM/wire candidate B

  candidate A/B + common PostgreSQL extension
          -> disposable PostgreSQL A/B -> Diesel/SeaORM/catalog parity

current transition:
  deployed P2 DDL -> locked JSON shadow -> generated TypeSpec/JSON/Protobuf
```

`schema/persistence.schema.json` is currently an imported shadow whose table,
column, type, nullability, interface revision, SeaORM source blobs, and
production SQL blob are checked elsewhere.
`tools/typespec-protobuf-parity.mjs` currently generates TypeSpec from that
shadow, invokes the pinned official JSON Schema and Protobuf emitters, and
independently parses both outputs. Before P1 promotion, this tree needs an
independence/provenance audit and a protected authored workflow; before P0
promotion, TypeSpec must move from generated evidence to the authored canonical
AST. Generated P0 JSON Schema must then live at a distinct path so it cannot
overwrite P1.

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

The current TypeSpec/Protobuf check is tied to the same source commit as the ORM
and schema Zed packages. A green check proves deterministic legacy
cross-projection at that commit; it does not prove independent P0/P1 agreement.

The target gate additionally requires independently reviewed P0 and P1 inputs,
normalized source/catalog/Diesel/SeaORM/behavior/wire parity, stable Protobuf
identity, and a reviewed expected-divergence registry. Neither emitter output
nor ORM code replaces the `declarative-migrations` plan against a fresh live
PostgreSQL catalog.
