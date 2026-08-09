import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { test } from "node:test";

import type {
  RegistryNamespaceProvider,
  RegistryNamespaceRequest,
} from "@zed-pkg/zed-interfaces";

import {
  planRegistryNamespaces,
  summarizeRegistryNamespacePlan,
} from "./namespace-plan.ts";

interface ExpectedEntry {
  readonly provider: RegistryNamespaceProvider;
  readonly coordinate: string | null;
  readonly package_prefix: string | null;
  readonly automation: string;
  readonly disposition: string;
  readonly proofs: readonly string[];
  readonly step_actions: readonly string[];
}

interface PlannerCase {
  readonly name: string;
  readonly request: RegistryNamespaceRequest;
  readonly expected: readonly ExpectedEntry[];
}

interface PlannerCorpus {
  readonly schema: "zed.registry-namespace-planner-cases/v1";
  readonly cases: readonly PlannerCase[];
}

const corpusPath = path.join(
  import.meta.dirname,
  "../../conformance/cases/registry-namespace-plans.json",
);
const corpus = JSON.parse(fs.readFileSync(corpusPath, "utf8")) as PlannerCorpus;

for (const testCase of corpus.cases) {
  test(`registry-namespace-plans.json: ${testCase.name}`, () => {
    const plan = planRegistryNamespaces(testCase.request);
    assert.deepEqual(summarizeRegistryNamespacePlan(plan), testCase.expected);
    assert.deepEqual(
      plan.request.providers,
      testCase.expected.map((entry) => entry.provider),
      "provider order must be canonical",
    );
  });
}

test("registry namespace planner rejects non-ASCII confusables", () => {
  assert.throws(() =>
    planRegistryNamespaces({
      brand: "acmе-cloud",
      domain: "acme.example",
      github_owner: "acme-cloud",
      providers: ["npm"],
    }),
  );
});

test("registry namespace planner rejects duplicate providers", () => {
  assert.throws(() =>
    planRegistryNamespaces({
      brand: "acme-cloud",
      domain: "acme.example",
      github_owner: "acme-cloud",
      providers: ["npm", "npm"],
    }),
  );
});
