import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

import { generateFiles, normalizeSchema } from '../tools/schema-shadow-codegen.mjs';
import { extractSqlTableBlock, gitBlobSha, parseSeaOrmEntity } from '../tools/check-schema-shadow.mjs';

const schema = JSON.parse(readFileSync(new URL('../schema/persistence.schema.json', import.meta.url), 'utf8'));

test('normalizes the complete imported Zed persistence surface', () => {
  const model = normalizeSchema(schema);
  assert.equal(model.entities.length, 15);
  assert.equal(model.contract.authorityMode, 'shadow-import');
  assert.equal(model.contract.productionSql, 'src/rust-orm/sql/registry.sql');
  assert.deepEqual(model.entityMap.get('OrgMember').primaryKey, ['orgId', 'userId']);
  assert.deepEqual(model.entityMap.get('ProjectMember').primaryKey, ['projectId', 'userId']);
});

test('emits every requested ORM family without migration authority', () => {
  const { files, manifest } = generateFiles(schema);
  const expected = [
    'rust/sea-orm/entities.rs',
    'node/drizzle/schema.ts',
    'node/prisma/schema.prisma',
    'node/typeorm/entities.ts',
    'go/gorm/models.go',
    'go/ent/schema/entities.go',
    'dart/drift/tables.dart',
    'dart/stormberry/models.dart',
    'sql/postgres.sql',
    'sql/sqlite.sql',
    'shared/typescript/entity-descriptors.ts',
    'shared/rust/entity_descriptors.rs',
    'shared/go/entity_descriptors.go',
    'shared/dart/entity_descriptors.dart',
  ];
  for (const path of expected) assert.ok(files.has(path), path);
  assert.equal(manifest.entityCount, 15);
  assert.equal(manifest.authorityMode, 'shadow-import');
  assert.match(files.get('sql/postgres.sql'), /SHADOW ONLY/);
  assert.match(files.get('sql/postgres.sql'), /src\/rust-orm\/sql\/registry\.sql/);
  assert.match(files.get('go/ent/schema/entities.go'), /ent\.View/);
  assert.match(files.get('go/ent/schema/entities.go'), /entsql\.Skip/);
});

test('pins public interface projections instead of redefining wire DTOs', () => {
  const interfaces = JSON.parse(readFileSync(new URL('../schema/interfaces.lock.json', import.meta.url), 'utf8'));
  const bindings = new Map(interfaces.bindings.map((binding) => [binding.entity, binding.interfaceType]));
  assert.equal(bindings.get('Package'), 'PackageMetadata');
  assert.equal(bindings.get('PackageVersion'), 'VersionMetadata');
  assert.equal(bindings.get('Org'), 'ClaimOrgResponse');
  assert.ok(interfaces.internalPersistenceEntities.includes('ApiToken'));
});

test('parses SeaORM and SQL shapes used by the drift gate', () => {
  const sea = parseSeaOrmEntity(`
#[sea_orm(table_name = "zed_example")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub optional_value: Option<String>,
}
`, 'fixture.rs');
  assert.equal(sea.table, 'zed_example');
  assert.equal(sea.fields.get('optional_value'), 'Option<String>');

  const block = extractSqlTableBlock(`
create table if not exists zed_example (
  id uuid not null,
  optional_value text,
  primary key (id)
);
`, 'zed_example');
  assert.match(block, /optional_value text/);
  assert.equal(gitBlobSha('abc'), 'f2ba8f84ab5c1bce84a7b441cb1959cfc7093b7f');
});
