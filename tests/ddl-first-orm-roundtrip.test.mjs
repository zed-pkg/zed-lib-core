import assert from 'node:assert/strict';
import test from 'node:test';

import {
  assertAuthoredDdlSemantics,
  assertGeneratedSqlIsNonAuthoritative,
  canonicalizeDrizzleTableMetadata,
  canonicalizeDrizzleTableOrder,
  normalizePulledDrizzleSchema,
  parseMode,
  stripDrizzleImplicitOperatorClasses,
  validateDisposableDatabaseUrl,
} from '../tools/ddl-first-orm-roundtrip.mjs';

test('allows only explicitly disposable loopback PostgreSQL databases', () => {
  assert.equal(
    validateDisposableDatabaseUrl('postgresql://worker@127.0.0.1:5432/zed_ddl_roundtrip_ci').database,
    'zed_ddl_roundtrip_ci',
  );
  for (const unsafe of [
    'postgresql://worker@db.example.com/zed_ddl_roundtrip_ci',
    'postgresql://worker@localhost/production',
    'mysql://worker@localhost/zed_ddl_roundtrip_ci',
  ]) {
    assert.throws(() => validateDisposableDatabaseUrl(unsafe));
  }
});

test('sorts introspected metadata by stable database object name', () => {
  const tables = Array.from({ length: 17 }, (_, index) => `
export const zed_t${index} = pgTable("zed_t${index}", { id: uuid() }, (table) => [
\tcheck("zed_t${index}_z_chk", sql\`id is not null\`),
\tforeignKey({
\t\tcolumns: [table.id],
\t\tforeignColumns: [zed_t0.id],
\t\tname: "zed_t${index}_m_fk"
\t}),
\tindex("zed_t${index}_a_idx").on(table.id),
]);`).join('\n');
  const canonical = canonicalizeDrizzleTableMetadata(tables);
  assert.ok(canonical.indexOf('zed_t0_a_idx') < canonical.indexOf('zed_t0_m_fk'));
  assert.ok(canonical.indexOf('zed_t0_m_fk') < canonical.indexOf('zed_t0_z_chk'));
  assert.equal(canonicalizeDrizzleTableMetadata(canonical), canonical);
});

test('sorts complete Drizzle table declarations without changing their bodies', () => {
  const tables = ['zed_z', 'zed_a', 'zed_m'].map((name) => `export const ${name} = pgTable("${name}", {
\tid: uuid().primaryKey(),
});`).join('\n\n');
  const canonical = canonicalizeDrizzleTableOrder(tables);
  assert.ok(canonical.indexOf('export const zed_a') < canonical.indexOf('export const zed_m'));
  assert.ok(canonical.indexOf('export const zed_m') < canonical.indexOf('export const zed_z'));
  assert.equal(canonicalizeDrizzleTableOrder(canonical), canonical);
});

test('removes only implicit Drizzle operator-class calls', () => {
  const source = 'index("idx").using("btree", table.id.asc().op("uuid_ops"))';
  assert.equal(stripDrizzleImplicitOperatorClasses(source), 'index("idx").using("btree", table.id.asc())');
  assert.throws(
    () => stripDrizzleImplicitOperatorClasses('index("idx").using("btree", table.id.asc().op(customOperator))'),
    /unrecognized operator-class expression/,
  );
});

test('repairs only the one known Drizzle empty-string introspection defect', () => {
  const source = "export const zed_packages = pgTable('zed_packages', {\n\trepo_url: text().default(').notNull(),\n});\n";
  const normalized = normalizePulledDrizzleSchema(source);
  assert.match(normalized, /SHADOW ONLY/);
  assert.match(normalized, /repo_url: text\(\)\.default\(''\)\.notNull\(\)/);
  assert.throws(() => normalizePulledDrizzleSchema(source.replace('repo_url', 'other_url')), /expected one repo_url repair/);
  assert.throws(() => normalizePulledDrizzleSchema(`${source}${source}`), /found 2 known and 2 total/);
});

test('keeps authored database behavior visibly outside generated SQL', () => {
  const authored = `
    create or replace function zed_guard() returns trigger language plpgsql as $$
    begin raise exception using errcode = 'ZD001'; end $$;
    -- ZD002 ZD003 ZD004 ZD005
    ${Array.from({ length: 17 }, (_, index) => `create table if not exists zed_t${index} (id uuid);`).join('\n')}
    create trigger zed_guard before update on zed_t0 execute function zed_guard();
  `;
  assert.equal(assertAuthoredDdlSemantics(authored), 17);
  assert.throws(() => assertAuthoredDdlSemantics(`${authored}\ncreate index x on zed_t0 (id text_pattern_ops);`), /explicit operator class/);
  const generated = Array.from({ length: 17 }, (_, index) => `CREATE TABLE "zed_t${index}" ("id" uuid);`).join('\n');
  assert.doesNotThrow(() => assertGeneratedSqlIsNonAuthoritative(generated));
  assert.throws(() => assertGeneratedSqlIsNonAuthoritative(`${generated}\nCREATE TRIGGER unsafe;`), /authored-only semantic/);
});

test('requires an explicit generation mode', () => {
  assert.equal(parseMode(['--check']), 'check');
  assert.equal(parseMode(['--write']), 'write');
  assert.throws(() => parseMode([]), /usage/);
  assert.throws(() => parseMode(['--write', '--check']), /usage/);
});
