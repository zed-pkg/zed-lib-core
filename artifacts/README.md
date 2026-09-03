# Dual-source parity evidence

Independent TypeSpec and JSON Schema/OpenAPI lanes write canonical JCS/NDJSON pairs under `interfaces-ir/`, `contract-ir/`, `persistence-ir/`, `sql-catalog/`, and `orm-ir/`. ORM IR here is comparison evidence only, never executable ORM code.

Bootstrap may omit evidence but cannot release. Once one pair exists, all pairs are required. `agreement.lock` is generated only after exact semantic equivalence.
