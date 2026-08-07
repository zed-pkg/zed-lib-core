// Runs every corpus file in ../../conformance/cases against the TypeScript
// implementation. The Rust and Dart slices load the same directory.
//
//   node --test conformance.test.ts
//
// Most of these cases were answered by Rust and written out by
// `cargo run --example generate_fuzz_corpus`: Rust delegates the hard part to
// the same `semver` crate Cargo uses, so what is really under test is whether
// this hand-written algebra agrees with Cargo across combinations nobody would
// think to write down by hand.
//
// No build step: Node strips the types on the way in.

import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { test } from "node:test";

import type { PackageMetadata, VersionScheme } from "@zed-pkg/zed-interfaces";

import { ResolveError, latestStable, resolveVersion } from "./index.ts";

const RESOLUTION_SCHEMA = "zed-lib/conformance/version-resolution/v1";
const LATEST_SCHEMA = "zed-lib/conformance/latest-stable/v1";

interface Case {
  readonly name: string;
  readonly scheme: string;
  readonly versions: readonly string[];
  readonly requirement?: string;
  readonly latest?: string | null;
  readonly expect: { readonly version?: string | null; readonly error?: string };
}

interface Corpus {
  readonly schema: string;
  readonly cases: readonly Case[];
}

const casesDir = path.join(import.meta.dirname, "../../conformance/cases");

/** `latest` is data for a latest-stable case — including when it is null, which
 *  is what "the registry recorded nothing" looks like. Resolution cases never
 *  read it, so they get a convenient fallback instead. */
function metadataFor(testCase: Case, latestIsData: boolean): PackageMetadata {
  const { versions } = testCase;
  const fallback = versions.length ? (versions[versions.length - 1] as string) : null;
  const scheme = testCase.scheme === "calver" || testCase.scheme === "opaque"
    ? testCase.scheme
    : "semver";
  return {
    org: "acme",
    name: "conformance",
    vcs: "git",
    repo_url: "https://github.com/acme/conformance",
    latest: latestIsData ? (testCase.latest ?? null) : (testCase.latest ?? fallback),
    versions,
    version_scheme: scheme as VersionScheme,
  };
}

const files = fs
  .readdirSync(casesDir)
  .filter((name) => name.endsWith(".json"))
  .sort();

test("the corpus directory is not empty", () => {
  assert.ok(files.length > 0);
});

let total = 0;

for (const file of files) {
  const corpus = JSON.parse(fs.readFileSync(path.join(casesDir, file), "utf8")) as Corpus;
  assert.ok(corpus.cases.length > 0, `${file} has no cases`);

  for (const testCase of corpus.cases) {
    total += 1;

    if (corpus.schema === RESOLUTION_SCHEMA) {
      test(`${file}: ${testCase.name}`, () => {
        const metadata = metadataFor(testCase, false);
        const requirement = testCase.requirement;
        assert.ok(requirement !== undefined, "resolution cases need a `requirement`");
        const want = testCase.expect.version;
        const wantError = testCase.expect.error;
        assert.ok(
          (want === undefined) !== (wantError === undefined),
          "declare exactly one of `version` or `error`",
        );
        if (want !== undefined) {
          assert.equal(resolveVersion(metadata, requirement), want);
          return;
        }
        assert.throws(
          () => resolveVersion(metadata, requirement),
          (error: unknown) => {
            assert.ok(error instanceof ResolveError);
            assert.equal(error.kind, wantError);
            return true;
          },
        );
      });
      continue;
    }

    if (corpus.schema === LATEST_SCHEMA) {
      test(`${file}: ${testCase.name}`, () => {
        assert.equal(testCase.expect.error, undefined, "latest-stable cases return null");
        assert.equal(latestStable(metadataFor(testCase, true)), testCase.expect.version ?? null);
      });
      continue;
    }

    throw new Error(`${file}: unknown corpus schema \`${corpus.schema}\``);
  }
}

// A loader bug that silently matched nothing would look like a clean run.
test("the generated corpus was loaded too", () => {
  assert.ok(total > 100, `ran only ${total} cases`);
});
