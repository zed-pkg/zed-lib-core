# Zed package graph document/edge atomic commit admission

Run `node formal/attolang/model_check.mjs`. The bounded state enumeration checks positive/negative witnesses and includes a deliberately broken guard as a mutation negative control.

`graph_policy.atto` is a source-only AttoLang oracle (draft `.atto` syntax); CI does NOT compile or execute this source or drive the production implementation.

The model covers document validation, normalized edges, digest admission and post-seal immutability. It cannot prove SHA-256 correctness, actual database transaction atomicity, production ORM enforcement, or Rust/TypeScript/Dart conformance.

To make stronger claims, add an exact-head production trace/refinement adapter and execute the `.atto` program on five real compiler backends (Java, LLVM, Wasm, Forge and .NET), with nonzero executed steps and reviewed receipts.
