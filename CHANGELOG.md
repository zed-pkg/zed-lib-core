# Changelog

## Unreleased

- Classify Windows `LockFileEx` error 33 (`ERROR_LOCK_VIOLATION`) as
  ordinary nonblocking contention, returning `Ok(None)` from
  `LockManager::try_acquire` instead of a hard I/O error. This matches the
  existing cross-platform meaning of a failed nonblocking acquisition: another
  descriptor remains authoritative and the caller may retry or wait.
- Retain `WouldBlock` behavior on every platform and continue to surface
  unrelated I/O failures.

## 0.1.1 — 2026-08-05

Repository and release-contract hardening for the standalone locking crate.

- Declare Rust 1.88 as the actual minimum supported compiler for the extracted
  let-chain implementation.
- Correct the Zed native registry identifier to `crates-io`.
- Remove the empty `.zpkg.lock` placeholder; the package currently has no Zed
  dependencies.
- Add fail-closed Cargo/Zed metadata, extraction-provenance, descriptor-lock,
  and production no-polling checks.
- Add negative package-contract tests.
- Run formatting, tests, strict Clippy, and process conformance on Ubuntu 24.04,
  macOS 15, and Windows Server 2025.
- Package a distinct `zed-lock-0.1.1.crate` and SHA-256 review artifact only
  after the complete platform matrix succeeds.
- Remove automatic release creation from ordinary default-branch pushes.

The locking API and extracted runtime implementation remain compatible with
0.1.0. Consumers should pin the immutable 0.1.1 merge/release commit rather
than retargeting the existing 0.1.0 tag.

## 0.1.0 — 2026-08-05

Initial standalone extraction of the kernel-backed, event-driven locking crate
from `zed-pkg/zed-cli` source commit
`fd3b08eb1ac170518cb795e662318ae2714b1176`.

The GitHub release targets commit
`0fc100afc3cd60b5ce091b4207f910bf08f2cfb7` and includes the original crate and
checksum assets.
