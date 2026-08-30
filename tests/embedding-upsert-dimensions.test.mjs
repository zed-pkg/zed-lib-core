import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import test from 'node:test';

const source = readFileSync(new URL('../src/rust-orm/registry/search.rs', import.meta.url), 'utf8');
const sql = source.match(/r#"\n(INSERT INTO zed_entity_embeddings[\s\S]*?)\n"#,/)?.[1];

test('the executed upsert guards the dimension inside ON CONFLICT', () => {
  assert.ok(sql, 'could not locate the runtime embedding upsert SQL');
  assert.match(sql, /ON CONFLICT \(entity_type, entity_id, embedding_model, content_sha256\)/);
  assert.match(sql, /DO UPDATE SET[\s\S]*WHERE zed_entity_embeddings\.embedding_dimensions = EXCLUDED\.embedding_dimensions\s+RETURNING id/);
});

test('a rejected upsert is a policy failure rather than successful replacement', () => {
  assert.match(source, /\.ok_or_else\(\|\| \{\s*OrmError::policy\(\s*"embedding dimensions conflict/);
});

const databaseUrl = process.env.EMBEDDING_TEST_DATABASE_URL;
const requirePostgres = process.env.EMBEDDING_REQUIRE_POSTGRES === '1';
test('PostgreSQL preserves the original row when dimensions conflict', {
  skip: !databaseUrl && !requirePostgres ? 'set EMBEDDING_TEST_DATABASE_URL to run the database regression' : false,
}, () => {
  assert.ok(databaseUrl, 'EMBEDDING_REQUIRE_POSTGRES=1 requires EMBEDDING_TEST_DATABASE_URL');
  assert.ok(sql, 'runtime SQL is required');
  const script = `
BEGIN;
SET LOCAL statement_timeout = '10s';
SET LOCAL search_path = pg_temp, pg_catalog;
CREATE TEMPORARY TABLE zed_entity_embeddings (
  id uuid PRIMARY KEY,
  entity_type text NOT NULL,
  entity_id uuid NOT NULL,
  org_id uuid NOT NULL,
  embedding_model text NOT NULL,
  embedding jsonb NOT NULL,
  embedding_dimensions integer NOT NULL CHECK (embedding_dimensions BETWEEN 1 AND 8192),
  content_sha256 text NOT NULL,
  content_preview text,
  updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  UNIQUE (entity_type, entity_id, embedding_model, content_sha256),
  CHECK (jsonb_array_length(embedding) = embedding_dimensions)
) ON COMMIT DROP;
PREPARE embedding_upsert (uuid, text, uuid, uuid, text, jsonb, integer, text, text) AS
${sql};
EXECUTE embedding_upsert(
  '00000000-0000-0000-0000-000000000001', 'package',
  '00000000-0000-0000-0000-000000000010', '00000000-0000-0000-0000-000000000100',
  'test/model-v1', '[1,0,0]'::jsonb, 3, repeat('a',64), 'original');
EXECUTE embedding_upsert(
  '00000000-0000-0000-0000-000000000002', 'package',
  '00000000-0000-0000-0000-000000000010', '00000000-0000-0000-0000-000000000100',
  'test/model-v1', '[0,1,0]'::jsonb, 3, repeat('a',64), 'same-dimension refresh');
DO $$ BEGIN
  IF NOT EXISTS (SELECT 1 FROM zed_entity_embeddings
    WHERE id = '00000000-0000-0000-0000-000000000001'
      AND embedding = '[0,1,0]'::jsonb AND embedding_dimensions = 3
      AND content_preview = 'same-dimension refresh') THEN
    RAISE EXCEPTION 'same-dimension upsert failed to retain the row identity';
  END IF;
END $$;
CREATE TEMPORARY TABLE embedding_before_conflict ON COMMIT DROP AS
  SELECT * FROM zed_entity_embeddings;
EXECUTE embedding_upsert(
  '00000000-0000-0000-0000-000000000003', 'package',
  '00000000-0000-0000-0000-000000000010', '00000000-0000-0000-0000-000000000999',
  'test/model-v1', '[1,0]'::jsonb, 2, repeat('a',64), 'must not replace');
DO $$ BEGIN
  IF EXISTS ((SELECT * FROM zed_entity_embeddings EXCEPT SELECT * FROM embedding_before_conflict)
     UNION ALL (SELECT * FROM embedding_before_conflict EXCEPT SELECT * FROM zed_entity_embeddings)) THEN
    RAISE EXCEPTION 'cross-dimension conflict changed the existing embedding';
  END IF;
END $$;
${[1536,3072,4096,8192].map((dimensions,index) => `
EXECUTE embedding_upsert(
  '00000000-0000-0000-0000-0000000000${20+index}', 'package',
  '00000000-0000-0000-0000-0000000000${30+index}', '00000000-0000-0000-0000-000000000100',
  'test/model-v1', to_jsonb(array_fill(1.0::real, ARRAY[${dimensions}])), ${dimensions},
  repeat('b',64), 'large-vector fixture');`).join('\n')}
DO $$ BEGIN
  IF (SELECT count(*) FROM zed_entity_embeddings) <> 5 THEN
    RAISE EXCEPTION 'large-vector inserts did not preserve separate entities';
  END IF;
  IF EXISTS (SELECT 1 FROM zed_entity_embeddings WHERE jsonb_array_length(embedding) <> embedding_dimensions) THEN
    RAISE EXCEPTION 'stored dimensions do not match the vector length';
  END IF;
END $$;
ROLLBACK;
`;
  const result = spawnSync('psql', ['-X', '--set=ON_ERROR_STOP=1', '--quiet', '--no-align', '--tuples-only'], {
    input: script,
    encoding: 'utf8',
    timeout: 25_000,
    maxBuffer: 1024 * 1024,
    env: { ...process.env, PGDATABASE: databaseUrl, PGCONNECT_TIMEOUT: '5' },
  });
  assert.ifError(result.error);
  assert.equal(result.status, 0, `PostgreSQL regression failed with exit status ${result.status}`);
});
