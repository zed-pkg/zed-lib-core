# Isomorphic validation SDK

This directory is the runtime validation projection for `zed-pkg/zed-lib-core`.

- Public definitions are authored in `zed-interfaces` and are safe for browser, mobile, desktop, CLI, and server consumers.
- Server definitions live only here. They may extend public definitions but are never copied into `zed-clients`.
- Route bindings use stable `operationId` values from `ORESoftware/api-docs`; validators do not invent a second route namespace.
- TypeSpec and JSON Schema/OpenAPI remain independent peer authorities. A mismatch is a stop-and-evaluate condition.

Runtime choices are Zod for TypeScript, Garde for Rust, `go-playground/validator/v10` for Go, and Gleam's official dynamic decoder API.
