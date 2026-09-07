import assert from 'node:assert/strict';
import { execFile } from 'node:child_process';
import { readFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { promisify } from 'node:util';
import test from 'node:test';
import { parsePublic, safeParsePublic } from '../typescript/dist/public.js';
import { checkPublicParity, VALIDATOR_REVISION } from '../../.deps/zed-interfaces/validation/compiler/public-parity.mjs';

const exec = promisify(execFile);
const root = resolve(import.meta.dirname, '../..');
const ids = { RequestMeta: 'request-meta', PageQuery: 'page-query', ProblemDetails: 'problem-details' };

test('TypeScript runtime matches the admitted production peer-authority corpus', async (t) => {
  const pins = JSON.parse(await readFile(resolve(import.meta.dirname, 'pins.json'), 'utf8'));
  assert.equal((await exec('git', ['-C', resolve(root, '.deps/zed-interfaces'), 'rev-parse', 'HEAD'])).stdout.trim(), pins.interfaces);
  assert.equal(VALIDATOR_REVISION, pins.validator);
  const evidence = await checkPublicParity();
  assert.equal(evidence.contractIr.admission.scope.complete, true);
  assert.equal(evidence.cases.length, 34);
  for (const item of evidence.cases) await t.test(`${item.declaration}/${item.id}`, () => {
    const candidateIds = item.declaration === 'PublicValidationContract' ? Object.values(ids) : [ids[item.declaration]];
    assert.ok(candidateIds.every(Boolean), 'unimplemented runtime declaration');
    const accepted = candidateIds.map((id) => safeParsePublic(id, item.value)).filter((result) => result.success);
    assert.equal(accepted.length === 1, item.valid, 'runtime disagrees with both admitted schema lanes');
    if (item.valid) assert.deepEqual(accepted[0].data, item.value, 'wire validation must not mutate accepted data');
  });
});

test('validation preserves whitespace rather than silently normalizing wire data', () => {
  const value = { requestId: ' r ', traceId: ' t ', locale: '  ' };
  assert.deepEqual(parsePublic('request-meta', value), value);
  assert.deepEqual(parsePublic('page-query', { limit: 50, cursor: ' ' }), { limit: 50, cursor: ' ' });
});

test('Unicode code-point bounds do not depend on UTF-16 or byte length', () => {
  assert.equal(safeParsePublic('request-meta', { requestId: '😀'.repeat(128), traceId: 't' }).success, true);
  assert.equal(safeParsePublic('request-meta', { requestId: '😀'.repeat(129), traceId: 't' }).success, false);
  assert.equal(safeParsePublic('request-meta', { requestId: 'r', traceId: 't', locale: 'e\u0301' }).success, true);
});
