import assert from 'node:assert/strict';
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { join, relative, resolve } from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

import { normalizeSchema } from '../tools/schema-shadow-codegen.mjs';
import {
  generateFiles,
  generateTypeSpec,
  projectionForField,
  updateFieldNumberLock,
  validateFieldNumberLock,
  verifyEmittedContracts,
} from '../tools/typespec-protobuf-parity.mjs';

const repositoryRoot = resolve(fileURLToPath(new URL('..', import.meta.url)));
const schema = JSON.parse(readFileSync(join(repositoryRoot, 'schema/persistence.schema.json'), 'utf8'));
const lock = JSON.parse(readFileSync(join(repositoryRoot, 'schema/typespec-protobuf.lock.json'), 'utf8'));

function committedEmittedFiles() {
  const root = join(repositoryRoot, 'generated/schema-contracts');
  const files = new Map();
  const walk = (directory) => {
    for (const entry of readdirSync(directory).sort()) {
      const path = join(directory, entry);
      if (statSync(path).isDirectory()) walk(path);
      else {
        const key = relative(root, path);
        if (key !== 'README.md' && key !== 'manifest.json' && key !== 'typespec/main.tsp') {
          files.set(key, readFileSync(path, 'utf8'));
        }
      }
    }
  };
  walk(root);
  return files;
}

test('locks all persistence fields to stable Protobuf identities', () => {
  const model = normalizeSchema(schema);
  validateFieldNumberLock(model, lock);
  assert.equal(model.entities.length, 17);
  assert.equal(model.entities.reduce((count, entity) => count + entity.fields.length, 0), 213);
  assert.equal(lock.package, 'zed.registry.v1');
  assert.equal(lock.messages.User.fields.id, 1);
  assert.equal(lock.messages.Package.fields.downloadCount, 12);
  assert.deepEqual(lock.messages.User.reserved, []);
});

test('emits and independently verifies JSON Schema and proto3 projections', () => {
  const model = normalizeSchema(schema);
  const emitted = committedEmittedFiles();
  verifyEmittedContracts(model, lock, emitted);

  const source = generateTypeSpec(model, lock);
  assert.match(source, /@package\(\{ name: "zed\.registry\.v1" \}\)/);
  assert.match(source, /@field\(12\)\n  downloadCount: int64;/);
  assert.match(source, /settings: bytes;/);
  assert.match(source, /email\?: string;/);

  const userJson = JSON.parse(emitted.get('json-schema/User.json'));
  assert.equal(userJson.properties.id.format, 'uuid');
  assert.equal(userJson.properties.settings.contentEncoding, 'base64');
  assert.ok(!userJson.required.includes('email'));
  assert.match(emitted.get('protobuf/zed/registry/v1.proto'), /optional string email = 4;/);
});

test('records every intentional representation transformation', () => {
  const { manifest } = generateFiles(schema, lock);
  assert.equal(manifest.transformations.counts['nullable-as-optional-presence'], 73);
  assert.equal(manifest.transformations.counts['json-object-as-opaque-bytes'], 8);
  assert.equal(manifest.transformations.counts['int64-as-json-decimal-string'], 5);
  assert.equal(manifest.transformations.counts['timestamp-as-rfc3339-string'], 42);
  assert.equal(projectionForField(normalizeSchema(schema).entityMap.get('Package').fieldMap.get('keywords')).proto.repeated, true);
});

test('preserves field numbers across order changes and reserves removals', () => {
  const reordered = normalizeSchema(schema);
  reordered.entities[0].fields.reverse();
  const reorderedLock = updateFieldNumberLock(reordered, lock);
  assert.equal(reorderedLock.messages.User.fields.id, lock.messages.User.fields.id);
  assert.equal(reorderedLock.messages.User.fields.updatedAt, lock.messages.User.fields.updatedAt);

  const removedSchema = structuredClone(schema);
  delete removedSchema.$defs.User.properties.avatarUrl;
  const removedModel = normalizeSchema(removedSchema);
  const removedLock = updateFieldNumberLock(removedModel, lock);
  assert.equal(removedLock.messages.User.fields.avatarUrl, undefined);
  assert.deepEqual(removedLock.messages.User.reserved, [{ name: 'avatarUrl', number: 6 }]);
  assert.throws(() => updateFieldNumberLock(normalizeSchema(schema), removedLock), /reuses a reserved Protobuf field name/);
});

test('rejects missing, duplicate, and implementation-reserved field numbers', () => {
  const model = normalizeSchema(schema);

  const missing = structuredClone(lock);
  delete missing.messages.User.fields.id;
  assert.throws(() => validateFieldNumberLock(model, missing), /field-number lock drift/);

  const duplicate = structuredClone(lock);
  duplicate.messages.User.fields.sharedAuthSubject = duplicate.messages.User.fields.id;
  assert.throws(() => validateFieldNumberLock(model, duplicate), /assigns Protobuf field number 1 more than once/);

  const forbidden = structuredClone(lock);
  forbidden.messages.User.fields.id = 19_000;
  assert.throws(() => validateFieldNumberLock(model, forbidden), /implementation-reserved range/);
});
