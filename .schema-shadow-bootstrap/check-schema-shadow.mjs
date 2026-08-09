#!/usr/bin/env node

import { createHash } from 'node:crypto';
import { existsSync, readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

function fail(message) {
  throw new Error(message);
}

function gitBlobSha(content) {
  const bytes = Buffer.from(content, 'utf8');
  return createHash('sha1').update(`blob ${bytes.length}\0`).update(bytes).digest('hex');
}

function snakeCase(value) {
  return value
    .replace(/([a-z0-9])([A-Z])/g, '$1_$2')
    .replace(/[-.\s]+/g, '_')
    .toLowerCase();
}

function parseSeaOrmEntity(content, source) {
  const table = content.match(/#\[sea_orm\(table_name\s*=\s*"([a-z0-9_]+)"\)\]/)?.[1];
  if (!table) fail(`${source}: missing #[sea_orm(table_name = ...)]`);
  const start = content.indexOf('pub struct Model {');
  if (start < 0) fail(`${source}: missing Model`);
  const after = content.slice(start + 'pub struct Model {'.length);
  const end = after.indexOf('\n}');
  if (end < 0) fail(`${source}: unterminated Model`);
  const block = after.slice(0, end);
  const fields = new Map();
  const pattern = /^\s*pub\s+([a-zA-Z0-9_]+)\s*:\s*([^,\n]+),/gm;
  for (const match of block.matchAll(pattern)) fields.set(match[1], match[2].trim());
  if (!fields.size) fail(`${source}: no Model fields found`);
  return { table, fields };
}

function extractSqlTableBlock(sql, table) {
  const escaped = table.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const marker = new RegExp(`create\\s+table(?:\\s+if\\s+not\\s+exists)?\\s+"?${escaped}"?\\s*\\(`, 'ig');
  const match = marker.exec(sql);
  if (!match) fail(`production SQL does not create ${table}`);
  const open = match.index + match[0].lastIndexOf('(');
  let depth = 0;
  let state = 'normal';
  let dollarTag = null;

  for (let index = open; index < sql.length; index += 1) {
    const char = sql[index];
    const next = sql[index + 1];

    if (state === 'line-comment') {
      if (char === '\n') state = 'normal';
      continue;
    }
    if (state === 'block-comment') {
      if (char === '*' && next === '/') {
        state = 'normal';
        index += 1;
      }
      continue;
    }
    if (state === 'single-quote') {
      if (char === "'" && next === "'") {
        index += 1;
      } else if (char === "'") {
        state = 'normal';
      }
      continue;
    }
    if (state === 'double-quote') {
      if (char === '"' && next === '"') {
        index += 1;
      } else if (char === '"') {
        state = 'normal';
      }
      continue;
    }
    if (state === 'dollar-quote') {
      if (dollarTag && sql.startsWith(dollarTag, index)) {
        index += dollarTag.length - 1;
        dollarTag = null;
        state = 'normal';
      }
      continue;
    }

    if (char === '-' && next === '-') {
      state = 'line-comment';
      index += 1;
      continue;
    }
    if (char === '/' && next === '*') {
      state = 'block-comment';
      index += 1;
      continue;
    }
    if (char === "'") {
      state = 'single-quote';
      continue;
    }
    if (char === '"') {
      state = 'double-quote';
      continue;
    }
    if (char === '$') {
      const tag = sql.slice(index).match(/^\$[A-Za-z_][A-Za-z0-9_]*\$|^\$\$/)?.[0];
      if (tag) {
        dollarTag = tag;
        state = 'dollar-quote';
        index += tag.length - 1;
        continue;
      }
    }
    if (char === '(') depth += 1;
    else if (char === ')') {
      depth -= 1;
      if (depth === 0) return sql.slice(open + 1, index);
    }
  }
  fail(`production SQL table ${table} is unterminated`);
}

function assertColumnInSql(block, table, column) {
  const escaped = column.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const pattern = new RegExp(`(?:^|\\n)\\s*"?${escaped}"?\\s+`, 'i');
  if (!pattern.test(block)) fail(`production SQL ${table} is missing column ${column}`);
}

function main() {
  const root = resolve(process.argv[2] ?? '.');
  const readJson = (path) => JSON.parse(readFileSync(resolve(root, path), 'utf8'));
  const schema = readJson('schema/persistence.schema.json');
  const importLock = readJson('schema/import.lock.json');
  const interfaceLock = readJson('schema/interfaces.lock.json');
  const contract = schema['x-lib-core'];

  if (contract.authorityMode !== 'shadow-import' || importLock.mode !== 'shadow-import') {
    fail('authority mode changed without the explicit promotion process');
  }
  if (contract.productionSql !== importLock.productionSql.path) fail('production SQL path lock mismatch');
  if (contract.productionSqlBlobSha !== importLock.productionSql.blobSha) fail('production SQL blob lock mismatch');
  if (contract.interfaces.repository !== interfaceLock.repository) fail('interface repository lock mismatch');
  if (contract.interfaces.revision !== interfaceLock.revision) fail('interface revision lock mismatch');
  if (contract.interfaces.schemasIndexBlobSha !== interfaceLock.schemasIndex.blobSha) fail('interface schema-index lock mismatch');
  if (!Array.isArray(importLock.unmodeledProductionFeatures) || importLock.unmodeledProductionFeatures.length < 5) {
    fail('the shadow contract must explicitly retain unmodeled production features');
  }

  const sqlPath = resolve(root, contract.productionSql);
  const productionSql = readFileSync(sqlPath, 'utf8');
  const actualSqlSha = gitBlobSha(productionSql);
  if (actualSqlSha !== contract.productionSqlBlobSha) {
    fail(`${contract.productionSql} moved from ${contract.productionSqlBlobSha} to ${actualSqlSha}; refresh the shadow import semantically`);
  }

  const lockedEntities = new Map(importLock.entities.map((entity) => [entity.entity, entity]));
  const schemaEntities = Object.entries(schema.$defs).filter(([, definition]) => definition['x-db']?.table);
  if (schemaEntities.length !== 15 || lockedEntities.size !== 15) fail('expected 15 imported persistence entities');

  let fieldCount = 0;
  for (const [name, definition] of schemaEntities) {
    const db = definition['x-db'];
    const locked = lockedEntities.get(name);
    if (!locked) fail(`${name} is not present in the import lock`);
    if (locked.table !== db.table || locked.source !== db.source || locked.blobSha !== db.sourceBlobSha) {
      fail(`${name} import metadata differs between schema and lock`);
    }

    const sourcePath = resolve(root, db.source);
    const source = readFileSync(sourcePath, 'utf8');
    const sourceSha = gitBlobSha(source);
    if (sourceSha !== db.sourceBlobSha) {
      fail(`${db.source} moved from ${db.sourceBlobSha} to ${sourceSha}; update JSON Schema and adapters in the same semantic change`);
    }

    const seaOrm = parseSeaOrmEntity(source, db.source);
    if (seaOrm.table !== db.table) fail(`${name}: schema table ${db.table} differs from SeaORM table ${seaOrm.table}`);
    const schemaFields = new Map(Object.entries(definition.properties).map(([propertyName, property]) => [
      property['x-db']?.column ?? snakeCase(propertyName),
      { propertyName, property },
    ]));
    const seaFields = new Set(seaOrm.fields.keys());
    const schemaColumns = new Set(schemaFields.keys());
    const missingFromSchema = [...seaFields].filter((column) => !schemaColumns.has(column));
    const missingFromSeaOrm = [...schemaColumns].filter((column) => !seaFields.has(column));
    if (missingFromSchema.length || missingFromSeaOrm.length) {
      fail(`${name}: field drift\nmissing from schema: ${missingFromSchema.join(', ') || '(none)'}\nmissing from SeaORM: ${missingFromSeaOrm.join(', ') || '(none)'}`);
    }

    const required = new Set(definition.required ?? []);
    for (const [column, { propertyName, property }] of schemaFields) {
      fieldCount += 1;
      const seaType = seaOrm.fields.get(column);
      const importedType = property['x-import']?.seaOrmType;
      if (!importedType) fail(`${name}.${propertyName}: x-import.seaOrmType is required`);
      if (seaType !== importedType) fail(`${name}.${propertyName}: expected ${importedType}, SeaORM has ${seaType}`);
      const schemaNullable = !required.has(propertyName) || (Array.isArray(property.type) && property.type.includes('null'));
      const seaNullable = seaType.startsWith('Option<');
      if (schemaNullable !== seaNullable) fail(`${name}.${propertyName}: nullability differs from SeaORM`);
    }

    const sqlBlock = extractSqlTableBlock(productionSql, db.table);
    for (const column of schemaColumns) assertColumnInSql(sqlBlock, db.table, column);
  }

  const generatedRoot = resolve(root, 'generated/schema-orm-shadow');
  const manifest = readJson('generated/schema-orm-shadow/manifest.json');
  if (manifest.authorityMode !== 'shadow-import') fail('generated manifest lost shadow authority mode');
  if (manifest.entityCount !== 15 || manifest.productionSqlBlobSha !== contract.productionSqlBlobSha) fail('generated manifest is stale');
  const generatedSql = readFileSync(resolve(generatedRoot, 'sql/postgres.sql'), 'utf8');
  if (!generatedSql.includes('SHADOW ONLY') || !generatedSql.includes(contract.productionSql)) fail('generated SQL lacks the non-executable shadow warning');
  if (existsSync(resolve(root, 'generated/sql/postgres.sql'))) fail('shadow output escaped into the canonical migration path');

  console.log(`verified ${schemaEntities.length} SeaORM entities, ${fieldCount} columns, production SQL blob ${actualSqlSha}, and isolated shadow adapters`);
}

const isMain = process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (isMain) {
  try {
    main();
  } catch (error) {
    console.error(error instanceof Error ? error.stack : String(error));
    process.exitCode = 1;
  }
}

export { extractSqlTableBlock, gitBlobSha, parseSeaOrmEntity };
