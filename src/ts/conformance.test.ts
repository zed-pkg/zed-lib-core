// Runs the shared conformance corpus against the TypeScript implementation.
// The Rust and Dart slices run the same file; a case that passes in one
// language and fails in another is the drift this repository exists to catch.
//
//   node --test conformance.test.ts
//
// No build step: Node strips the types on the way in.

import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { test } from "node:test";

import type { PackageMetadata, VersionScheme } from "@zed-pkg/zed-interfaces";

import { ResolveError, resolveVersion } from "./index.ts";

interface Case {
  readonly name: string;
  readonly scheme: string;
  readonly versions: readonly string[];
  readonly requirement: string;
  readonly expect: { readonly version?: string; readonly error?: string };
}

const corpusPath = path.join(
  import.meta.dirname,
  "../../conformance/cases/version-resolution.json",
);
const corpus = JSON.parse(fs.readFileSync(corpusPath, "utf8")) as { cases: Case[] };

const metadataFor = (scheme: string, versions: readonly string[]): PackageMetadata => ({
  org: "acme",
  name: "conformance",
  vcs: "git",
  repo_url: "https://github.com/acme/conformance",
  latest: versions.length ? (versions[versions.length - 1] as string) : null,
  versions,
  version_scheme: (scheme === "calver" || scheme === "opaque" ? scheme : "semver") as VersionScheme,
});

test("the corpus is not empty", () => {
  assert.ok(corpus.cases.length > 0);
});

for (const testCase of corpus.cases) {
  test(testCase.name, () => {
    const metadata = metadataFor(testCase.scheme, testCase.versions);
    const { version: want, error: wantError } = testCase.expect;
    assert.ok(
      (want === undefined) !== (wantError === undefined),
      "a case declares exactly one of `version` or `error`",
    );

    if (want !== undefined) {
      assert.equal(resolveVersion(metadata, testCase.requirement), want);
      return;
    }
    assert.throws(
      () => resolveVersion(metadata, testCase.requirement),
      (error: unknown) => {
        assert.ok(error instanceof ResolveError);
        assert.equal(error.kind, wantError);
        return true;
      },
    );
  });
}
