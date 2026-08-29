#!/usr/bin/env node

import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { createRequire } from 'node:module';
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
  statSync,
  writeFileSync,
} from 'node:fs';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { normalizeSchema, stableStringify } from './schema-shadow-codegen.mjs';

const ENTRY_PATH = fileURLToPath(import.meta.url);
const REPOSITORY_ROOT = resolve(dirname(ENTRY_PATH), '..');
const TOOLING_ROOT = join(REPOSITORY_ROOT, 'schema-tooling');
const SCHEMA_PATH = join(REPOSITORY_ROOT, 'schema', 'persistence.schema.json');
const FIELD_LOCK_PATH = join(REPOSITORY_ROOT, 'schema', 'typespec-protobuf.lock.json');
const OUTPUT_ROOT = join(REPOSITORY_ROOT, 'generated', 'schema-contracts');
const TYPESPEC_PATH = join(TOOLING_ROOT, 'node_modules', '.bin', 'tsp');
const TYPESPEC_PACKAGE = 'zed.registry.v1';
const TYPESPEC_NAMESPACE = 'ZedRegistry';
const TYPESPEC_IDENTIFIER = /^[A-Za-z_][A-Za-z0-9_]*$/;
const PROTOBUF_MAX_FIELD_NUMBER = 536_870_911;
const PROTOBUF_RESERVED_START = 19_000;
const PROTOBUF_RESERVED_END = 19_999;
const requireFromTooling = createRequire(join(TOOLING_ROOT, 'package.json'));

function fail(message) {
  throw new Error(message);
}

function sha256(content) {
  return createHash('sha256').update(content).digest('hex');
}

function assertSafeFieldNumber(value, label) {
  if (!Number.isInteger(value) || value < 1 || value > PROTOBUF_MAX_FIELD_NUMBER) {
    fail(`${label} must be an integer between 1 and ${PROTOBUF_MAX_FIELD_NUMBER}`);
  }
  if (value >= PROTOBUF_RESERVED_START && value <= PROTOBUF_RESERVED_END) {
    fail(`${label} uses Protobuf's implementation-reserved range`);
  }
}

function projectionShape(model) {
  return model.entities.map((entity) => ({
    fields: entity.fields.map((field) => ({
      format: field.property.format ?? null,
      items: field.property.items?.type ?? null,
      kind: field.kind,
      name: field.name,
      nullable: field.nullable,
    })),
    name: entity.name,
  }));
}

function shapeSha256(model) {
  return sha256(stableStringify(projectionShape(model)));
}

function emptyFieldNumberLock(model) {
  return {
    formatVersion: 1,
    messages: {},
    package: TYPESPEC_PACKAGE,
    retiredMessages: {},
    source: 'schema/persistence.schema.json',
    sourceShapeSha256: shapeSha256(model),
  };
}

function nextAvailableFieldNumber(message) {
  const occupied = new Set([
    ...Object.values(message.fields ?? {}),
    ...(message.reserved ?? []).map((reservation) => reservation.number),
  ]);
  let candidate = occupied.size ? Math.max(...occupied) + 1 : 1;
  if (candidate >= PROTOBUF_RESERVED_START && candidate <= PROTOBUF_RESERVED_END) {
    candidate = PROTOBUF_RESERVED_END + 1;
  }
  assertSafeFieldNumber(candidate, 'allocated field number');
  return candidate;
}

function updateFieldNumberLock(model, previous) {
  const lock = structuredClone(previous ?? emptyFieldNumberLock(model));
  if (lock.formatVersion !== 1) fail('field-number lock has an unsupported formatVersion');
  if (lock.package !== TYPESPEC_PACKAGE) fail(`field-number lock package must be ${TYPESPEC_PACKAGE}`);
  if (lock.source !== 'schema/persistence.schema.json') fail('field-number lock source moved unexpectedly');
  lock.messages ??= {};
  lock.retiredMessages ??= {};

  const activeNames = new Set(model.entities.map((entity) => entity.name));
  for (const name of Object.keys(lock.messages)) {
    if (activeNames.has(name)) continue;
    if (lock.retiredMessages[name]) fail(`${name} is active and retired in the field-number lock`);
    lock.retiredMessages[name] = lock.messages[name];
    delete lock.messages[name];
  }

  for (const entity of model.entities) {
    if (!TYPESPEC_IDENTIFIER.test(entity.name)) fail(`${entity.name} is not a safe TypeSpec message identifier`);
    if (lock.retiredMessages[entity.name]) {
      fail(`${entity.name} was retired; reusing a Protobuf message name requires a new protocol package`);
    }
    const message = lock.messages[entity.name] ?? { fields: {}, reserved: [] };
    message.fields ??= {};
    message.reserved ??= [];
    const currentFields = new Set(entity.fields.map((field) => field.name));

    for (const [name, number] of Object.entries(message.fields)) {
      if (currentFields.has(name)) continue;
      message.reserved.push({ name, number });
      delete message.fields[name];
    }

    for (const field of entity.fields) {
      if (!TYPESPEC_IDENTIFIER.test(field.name)) {
        fail(`${entity.name}.${field.name} is not a safe TypeSpec field identifier`);
      }
      if (message.fields[field.name] !== undefined) continue;
      if (message.reserved.some((reservation) => reservation.name === field.name)) {
        fail(`${entity.name}.${field.name} reuses a reserved Protobuf field name`);
      }
      message.fields[field.name] = nextAvailableFieldNumber(message);
    }

    message.reserved.sort((left, right) => left.number - right.number || left.name.localeCompare(right.name));
    lock.messages[entity.name] = message;
  }

  lock.sourceShapeSha256 = shapeSha256(model);
  validateFieldNumberLock(model, lock);
  return lock;
}

function validateMessageLock(name, message, expectedFields = null) {
  if (!message || typeof message !== 'object' || Array.isArray(message)) fail(`${name} lock entry must be an object`);
  if (!message.fields || typeof message.fields !== 'object' || Array.isArray(message.fields)) {
    fail(`${name}.fields must be an object`);
  }
  if (!Array.isArray(message.reserved)) fail(`${name}.reserved must be an array`);

  const activeNames = new Set();
  const activeNumbers = new Set();
  for (const [field, number] of Object.entries(message.fields)) {
    if (!TYPESPEC_IDENTIFIER.test(field)) fail(`${name}.${field} is not a safe Protobuf field name`);
    if (activeNames.has(field)) fail(`${name} has duplicate field name ${field}`);
    assertSafeFieldNumber(number, `${name}.${field}`);
    if (activeNumbers.has(number)) fail(`${name} assigns Protobuf field number ${number} more than once`);
    activeNames.add(field);
    activeNumbers.add(number);
  }

  const reservedNames = new Set();
  const reservedNumbers = new Set();
  for (const [index, reservation] of message.reserved.entries()) {
    if (!reservation || typeof reservation !== 'object' || Array.isArray(reservation)) {
      fail(`${name}.reserved[${index}] must bind one name and number`);
    }
    if (!TYPESPEC_IDENTIFIER.test(reservation.name)) fail(`${name} has an unsafe reserved name`);
    assertSafeFieldNumber(reservation.number, `${name}.reserved[${index}]`);
    if (activeNames.has(reservation.name) || reservedNames.has(reservation.name)) {
      fail(`${name} reuses reserved field name ${reservation.name}`);
    }
    if (activeNumbers.has(reservation.number) || reservedNumbers.has(reservation.number)) {
      fail(`${name} reuses reserved field number ${reservation.number}`);
    }
    reservedNames.add(reservation.name);
    reservedNumbers.add(reservation.number);
  }

  if (expectedFields) {
    const expected = [...expectedFields].sort();
    const actual = [...activeNames].sort();
    if (JSON.stringify(expected) !== JSON.stringify(actual)) {
      fail(`${name} field-number lock drift\nexpected: ${expected.join(', ')}\nactual: ${actual.join(', ')}`);
    }
  }
}

function validateFieldNumberLock(model, lock) {
  if (lock.formatVersion !== 1) fail('field-number lock formatVersion must be 1');
  if (lock.package !== TYPESPEC_PACKAGE) fail(`field-number lock package must be ${TYPESPEC_PACKAGE}`);
  if (lock.source !== 'schema/persistence.schema.json') fail('field-number lock source moved unexpectedly');
  if (lock.sourceShapeSha256 !== shapeSha256(model)) fail('field-number lock is stale for the persistence shape');
  if (!lock.messages || !lock.retiredMessages) fail('field-number lock must contain messages and retiredMessages');

  const expectedMessages = model.entities.map((entity) => entity.name).sort();
  const actualMessages = Object.keys(lock.messages).sort();
  if (JSON.stringify(expectedMessages) !== JSON.stringify(actualMessages)) {
    fail(`field-number message drift\nexpected: ${expectedMessages.join(', ')}\nactual: ${actualMessages.join(', ')}`);
  }
  for (const entity of model.entities) {
    validateMessageLock(entity.name, lock.messages[entity.name], entity.fields.map((field) => field.name));
    if (lock.retiredMessages[entity.name]) fail(`${entity.name} cannot be both active and retired`);
  }
  for (const [name, message] of Object.entries(lock.retiredMessages)) {
    if (!TYPESPEC_IDENTIFIER.test(name)) fail(`${name} is not a safe retired message name`);
    validateMessageLock(`retiredMessages.${name}`, message);
  }
  return lock;
}

function projectionForField(field) {
  if (field.kind === 'string') {
    return {
      decorators: field.property.format ? [`@format(${JSON.stringify(field.property.format)})`] : [],
      json: { type: 'string', format: field.property.format ?? null },
      proto: { type: 'string', repeated: false },
      typeSpec: 'string',
      transformation: field.property.format === 'date-time' ? 'timestamp-as-rfc3339-string' : null,
    };
  }
  if (field.kind === 'integer') {
    if (field.property.format === 'int32') {
      return {
        decorators: [],
        json: { type: 'integer', format: null },
        proto: { type: 'int32', repeated: false },
        typeSpec: 'int32',
        transformation: null,
      };
    }
    return {
      decorators: [],
      json: { type: 'string', format: null },
      proto: { type: 'int64', repeated: false },
      typeSpec: 'int64',
      transformation: 'int64-as-json-decimal-string',
    };
  }
  if (field.kind === 'boolean') {
    return {
      decorators: [],
      json: { type: 'boolean', format: null },
      proto: { type: 'bool', repeated: false },
      typeSpec: 'boolean',
      transformation: null,
    };
  }
  if (field.kind === 'object') {
    return {
      decorators: [],
      json: { type: 'string', format: null, contentEncoding: 'base64' },
      proto: { type: 'bytes', repeated: false },
      typeSpec: 'bytes',
      transformation: 'json-object-as-opaque-bytes',
    };
  }
  if (field.kind === 'array') {
    const item = field.property.items?.type;
    if (item === 'string') {
      return {
        decorators: [],
        json: { type: 'array', items: { type: 'string', format: null } },
        proto: { type: 'string', repeated: true },
        typeSpec: 'string[]',
        transformation: null,
      };
    }
    if (item === 'number') {
      return {
        decorators: [],
        json: { type: 'array', items: { type: 'number', format: null } },
        proto: { type: 'double', repeated: true },
        typeSpec: 'float64[]',
        transformation: null,
      };
    }
    fail(`${field.name}: unsupported array item type ${item}`);
  }
  fail(`${field.name}: unsupported TypeSpec projection kind ${field.kind}`);
}

function reservationDecorator(message) {
  if (!message.reserved.length) return null;
  const values = message.reserved.flatMap((reservation) => [String(reservation.number), JSON.stringify(reservation.name)]);
  return `@reserve(${values.join(', ')})`;
}

function generateTypeSpec(model, lock) {
  validateFieldNumberLock(model, lock);
  const lines = [
    '// Generated by tools/typespec-protobuf-parity.mjs. DO NOT EDIT.',
    '// SHADOW ONLY. Authored PostgreSQL DDL remains migration authority.',
    '// JSONB is deliberately projected as opaque bytes; nullable fields use optional presence.',
    '',
    'import "@typespec/json-schema";',
    'import "@typespec/protobuf";',
    '',
    'using TypeSpec.JsonSchema;',
    'using TypeSpec.Protobuf;',
    '',
    '@jsonSchema',
    `@package({ name: ${JSON.stringify(TYPESPEC_PACKAGE)} })`,
    `namespace ${TYPESPEC_NAMESPACE};`,
    '',
  ];

  for (const entity of model.entities) {
    const message = lock.messages[entity.name];
    const reservation = reservationDecorator(message);
    if (reservation) lines.push(reservation);
    lines.push('@message', `model ${entity.name} {`);
    for (const field of entity.fields) {
      const projection = projectionForField(field);
      lines.push(`  @field(${message.fields[field.name]})`);
      for (const decorator of projection.decorators) lines.push(`  ${decorator}`);
      const optional = field.nullable ? '?' : '';
      lines.push(`  ${field.name}${optional}: ${projection.typeSpec};`, '');
    }
    if (lines.at(-1) === '') lines.pop();
    lines.push('}', '');
  }
  return `${lines.join('\n').trimEnd()}\n`;
}

function typeSpecConfig() {
  return `emit:
  - "@typespec/json-schema"
  - "@typespec/protobuf"
options:
  "@typespec/json-schema":
    emitter-output-dir: "{output-dir}/json-schema"
    file-type: json
    emitAllModels: true
    int64-strategy: string
    seal-object-schemas: true
  "@typespec/protobuf":
    emitter-output-dir: "{output-dir}/protobuf"
`;
}

function listFiles(root) {
  if (!existsSync(root)) return [];
  const files = [];
  const walk = (directory) => {
    for (const entry of readdirSync(directory).sort()) {
      const path = join(directory, entry);
      if (statSync(path).isDirectory()) walk(path);
      else files.push(relative(root, path));
    }
  };
  walk(root);
  return files;
}

function command(binary, args, options = {}) {
  const result = spawnSync(binary, args, {
    cwd: options.cwd ?? REPOSITORY_ROOT,
    encoding: 'utf8',
    env: options.env ?? process.env,
    maxBuffer: 32 * 1024 * 1024,
  });
  if (result.error?.code === 'ENOENT') fail(`${options.label ?? binary} is required`);
  if (result.status !== 0) {
    fail(`${options.label ?? binary} failed: ${(result.stderr || result.stdout || result.error || 'unknown error').toString().trim()}`);
  }
  return result.stdout;
}

function formatTypeSpec(source) {
  if (!existsSync(TYPESPEC_PATH)) fail('pinned TypeSpec compiler is missing; run npm ci --prefix schema-tooling');
  const temporaryRoot = mkdtempSync(join(TOOLING_ROOT, '.typespec-format-'));
  const sourcePath = join(temporaryRoot, 'main.tsp');
  try {
    writeFileSync(sourcePath, source, 'utf8');
    command(TYPESPEC_PATH, ['format', sourcePath], { cwd: TOOLING_ROOT, label: 'TypeSpec formatter' });
    return readFileSync(sourcePath, 'utf8');
  } finally {
    if (!temporaryRoot.startsWith(`${TOOLING_ROOT}/.typespec-format-`)) fail('unsafe TypeSpec format path');
    rmSync(temporaryRoot, { recursive: true, force: true });
  }
}

function compileTypeSpec(source) {
  if (!existsSync(TYPESPEC_PATH)) fail('pinned TypeSpec compiler is missing; run npm ci --prefix schema-tooling');
  const temporaryRoot = mkdtempSync(join(TOOLING_ROOT, '.typespec-parity-'));
  const sourcePath = join(temporaryRoot, 'main.tsp');
  const configPath = join(temporaryRoot, 'tspconfig.yaml');
  const outputPath = join(temporaryRoot, 'output');
  try {
    writeFileSync(sourcePath, source, 'utf8');
    writeFileSync(configPath, typeSpecConfig(), 'utf8');
    command(TYPESPEC_PATH, ['compile', sourcePath, '--config', configPath, '--output-dir', outputPath], {
      cwd: TOOLING_ROOT,
      label: 'TypeSpec compiler',
    });
    const files = new Map();
    for (const path of listFiles(outputPath)) {
      files.set(path, readFileSync(join(outputPath, path), 'utf8'));
    }
    return files;
  } finally {
    if (!temporaryRoot.startsWith(`${TOOLING_ROOT}/.typespec-parity-`)) fail('unsafe TypeSpec temporary path');
    rmSync(temporaryRoot, { recursive: true, force: true });
  }
}

function assertJsonProperty(field, actual) {
  const expected = projectionForField(field).json;
  if (actual?.type !== expected.type) fail(`${field.name}: JSON Schema type is ${actual?.type}, expected ${expected.type}`);
  if ((actual.format ?? null) !== (expected.format ?? null)) {
    fail(`${field.name}: JSON Schema format is ${actual.format ?? '(none)'}, expected ${expected.format ?? '(none)'}`);
  }
  if ((actual.contentEncoding ?? null) !== (expected.contentEncoding ?? null)) {
    fail(`${field.name}: JSON Schema contentEncoding drifted`);
  }
  if (expected.items) {
    if (actual.items?.type !== expected.items.type || (actual.items?.format ?? null) !== (expected.items.format ?? null)) {
      fail(`${field.name}: JSON Schema array item projection drifted`);
    }
  }
}

function parseProtoMessages(source) {
  const messages = new Map();
  const messagePattern = /^message\s+([A-Za-z_][A-Za-z0-9_]*)\s*\{([\s\S]*?)^\}/gm;
  for (const match of source.matchAll(messagePattern)) {
    const fields = new Map();
    const reservedNames = new Set();
    const reservedNumbers = new Set();
    for (const line of match[2].split('\n')) {
      const field = line.match(/^\s*(?:(optional|repeated)\s+)?([A-Za-z_.][A-Za-z0-9_.]*)\s+([A-Za-z_][A-Za-z0-9_]*)\s*=\s*(\d+);\s*$/);
      if (field) {
        const [, label = '', type, name, rawNumber] = field;
        if (fields.has(name)) fail(`${match[1]} has duplicate emitted field ${name}`);
        fields.set(name, { label, number: Number(rawNumber), type });
        continue;
      }
      const reserved = line.match(/^\s*reserved\s+(.+);\s*$/);
      if (!reserved) continue;
      for (const value of reserved[1].split(',').map((item) => item.trim())) {
        if (/^\d+$/.test(value)) reservedNumbers.add(Number(value));
        else if (/^"[A-Za-z_][A-Za-z0-9_]*"$/.test(value)) reservedNames.add(JSON.parse(value));
        else fail(`${match[1]} has an unsupported reservation ${value}`);
      }
    }
    messages.set(match[1], { fields, reservedNames, reservedNumbers });
  }
  return messages;
}

function verifyEmittedContracts(model, lock, emitted) {
  const expectedJsonFiles = model.entities.map((entity) => `json-schema/${entity.name}.json`).sort();
  const actualJsonFiles = [...emitted.keys()].filter((path) => path.startsWith('json-schema/')).sort();
  if (JSON.stringify(expectedJsonFiles) !== JSON.stringify(actualJsonFiles)) {
    fail(`TypeSpec JSON Schema file drift\nexpected: ${expectedJsonFiles.join(', ')}\nactual: ${actualJsonFiles.join(', ')}`);
  }

  for (const entity of model.entities) {
    const document = JSON.parse(emitted.get(`json-schema/${entity.name}.json`));
    if (document.$schema !== 'https://json-schema.org/draft/2020-12/schema') fail(`${entity.name}: wrong JSON Schema dialect`);
    if (document.type !== 'object') fail(`${entity.name}: emitted JSON Schema is not an object`);
    if (stableStringify(document.unevaluatedProperties) !== stableStringify({ not: {} })) {
      fail(`${entity.name}: emitted JSON Schema is not sealed`);
    }
    const expectedFields = entity.fields.map((field) => field.name).sort();
    const actualFields = Object.keys(document.properties ?? {}).sort();
    if (JSON.stringify(expectedFields) !== JSON.stringify(actualFields)) fail(`${entity.name}: emitted JSON field set drifted`);
    const expectedRequired = entity.fields.filter((field) => !field.nullable).map((field) => field.name).sort();
    const actualRequired = [...(document.required ?? [])].sort();
    if (JSON.stringify(expectedRequired) !== JSON.stringify(actualRequired)) fail(`${entity.name}: emitted JSON required set drifted`);
    for (const field of entity.fields) assertJsonProperty(field, document.properties[field.name]);
  }

  const protoPath = `protobuf/${TYPESPEC_PACKAGE.replaceAll('.', '/')}.proto`;
  const proto = emitted.get(protoPath);
  if (!proto) fail(`TypeSpec did not emit ${protoPath}`);
  if (!proto.includes('syntax = "proto3";') || !proto.includes(`package ${TYPESPEC_PACKAGE};`)) {
    fail('emitted Protobuf syntax or package drifted');
  }
  const protobuf = requireFromTooling('protobufjs');
  try {
    protobuf.parse(proto, { keepCase: true });
  } catch (error) {
    fail(`protobufjs rejected the emitted proto: ${error instanceof Error ? error.message : String(error)}`);
  }

  const messages = parseProtoMessages(proto);
  const expectedMessages = model.entities.map((entity) => entity.name).sort();
  const actualMessages = [...messages.keys()].sort();
  if (JSON.stringify(expectedMessages) !== JSON.stringify(actualMessages)) fail('emitted Protobuf message set drifted');
  for (const entity of model.entities) {
    const actual = messages.get(entity.name);
    const messageLock = lock.messages[entity.name];
    const expectedFields = entity.fields.map((field) => field.name).sort();
    const actualFields = [...actual.fields.keys()].sort();
    if (JSON.stringify(expectedFields) !== JSON.stringify(actualFields)) fail(`${entity.name}: emitted Protobuf field set drifted`);
    for (const field of entity.fields) {
      const expected = projectionForField(field).proto;
      const emittedField = actual.fields.get(field.name);
      if (emittedField.number !== messageLock.fields[field.name]) fail(`${entity.name}.${field.name}: field number drifted`);
      if (emittedField.type !== expected.type) fail(`${entity.name}.${field.name}: Protobuf type drifted`);
      const expectedLabel = expected.repeated ? 'repeated' : field.nullable ? 'optional' : '';
      if (emittedField.label !== expectedLabel) fail(`${entity.name}.${field.name}: Protobuf presence label drifted`);
    }
    const expectedReservedNames = new Set(messageLock.reserved.map((reservation) => reservation.name));
    const expectedReservedNumbers = new Set(messageLock.reserved.map((reservation) => reservation.number));
    if (stableStringify([...actual.reservedNames].sort()) !== stableStringify([...expectedReservedNames].sort())) {
      fail(`${entity.name}: emitted reserved field names drifted`);
    }
    if (stableStringify([...actual.reservedNumbers].sort((a, b) => a - b)) !== stableStringify([...expectedReservedNumbers].sort((a, b) => a - b))) {
      fail(`${entity.name}: emitted reserved field numbers drifted`);
    }
  }
  return protoPath;
}

function transformationEvidence(model) {
  const fields = [];
  const counts = {};
  for (const entity of model.entities) {
    for (const field of entity.fields) {
      const transformations = [projectionForField(field).transformation];
      if (field.nullable) transformations.push('nullable-as-optional-presence');
      for (const transformation of transformations.filter(Boolean)) {
        counts[transformation] = (counts[transformation] ?? 0) + 1;
        fields.push({ entity: entity.name, field: field.name, transformation });
      }
    }
  }
  return { counts, fields };
}

function generatedReadme() {
  return `# TypeSpec and Protobuf shadow contracts

Generated from \`schema/persistence.schema.json\` and its stable Protobuf field-number lock. These are cross-check artifacts only. Authored PostgreSQL DDL in \`src/rust-orm/sql/**\` remains migration authority. Do not hand-edit or use these files to apply database changes.
`;
}

function generateFiles(schema, lock) {
  const model = normalizeSchema(schema);
  validateFieldNumberLock(model, lock);
  const source = formatTypeSpec(generateTypeSpec(model, lock));
  const emitted = compileTypeSpec(source);
  const protoPath = verifyEmittedContracts(model, lock, emitted);
  const files = new Map([
    ['README.md', generatedReadme()],
    ['typespec/main.tsp', source],
    ...emitted,
  ]);
  const transformations = transformationEvidence(model);
  const fieldCount = model.entities.reduce((count, entity) => count + entity.fields.length, 0);
  const nullableFieldCount = model.entities.reduce(
    (count, entity) => count + entity.fields.filter((field) => field.nullable).length,
    0,
  );
  const packageJson = JSON.parse(readFileSync(join(TOOLING_ROOT, 'package.json'), 'utf8'));
  const manifest = {
    authorityMode: model.contract.authorityMode,
    entityCount: model.entities.length,
    fieldCount,
    fieldNumberLock: 'schema/typespec-protobuf.lock.json',
    fieldNumberLockSha256: sha256(stableStringify(lock)),
    generator: 'tools/typespec-protobuf-parity.mjs',
    generatorFormatVersion: 1,
    generatorSha256: sha256(readFileSync(ENTRY_PATH, 'utf8')),
    jsonSchemaDialect: 'https://json-schema.org/draft/2020-12/schema',
    nullableFieldCount,
    outputs: Object.fromEntries([...files].map(([path, content]) => [path, sha256(content)])),
    productionSql: model.contract.productionSql,
    productionSqlBlobSha: model.contract.productionSqlBlobSha,
    protobufPackage: TYPESPEC_PACKAGE,
    protobufPath: protoPath,
    schemaSha256: sha256(stableStringify(schema)),
    source: 'schema/persistence.schema.json',
    toolchain: {
      compiler: packageJson.devDependencies['@typespec/compiler'],
      jsonSchemaEmitter: packageJson.devDependencies['@typespec/json-schema'],
      protobufEmitter: packageJson.devDependencies['@typespec/protobuf'],
      protobufParser: packageJson.devDependencies.protobufjs,
    },
    transformations,
    warnings: [
      'SHADOW ONLY. Authored PostgreSQL DDL remains migration authority.',
      'TypeSpec is generated from the locked persistence shadow; it is not an independently authored schema.',
      'Nullable persistence fields project to optional presence; explicit JSON null is not preserved.',
      'Proto3 does not enforce required presence for non-optional scalar fields.',
      'JSON object contents project to opaque bytes and are not semantically cross-checked by Protobuf.',
    ],
  };
  files.set('manifest.json', stableStringify(manifest));
  return { files, lock, manifest, model };
}

function assertOutputRoot(root) {
  if (resolve(root) !== OUTPUT_ROOT) fail(`refusing to replace generated files outside ${OUTPUT_ROOT}`);
  return OUTPUT_ROOT;
}

function writeGenerated(files, root) {
  assertOutputRoot(root);
  rmSync(root, { recursive: true, force: true });
  for (const [path, content] of files) {
    const target = join(root, path);
    mkdirSync(dirname(target), { recursive: true });
    writeFileSync(target, content, 'utf8');
  }
}

function checkGenerated(files, root) {
  const expected = [...files.keys()].sort();
  const actual = listFiles(root);
  const failures = [];
  if (JSON.stringify(expected) !== JSON.stringify(actual)) {
    failures.push(`generated contract file set drift\nexpected: ${expected.join(', ')}\nactual: ${actual.join(', ')}`);
  }
  for (const [path, content] of files) {
    const target = join(root, path);
    if (existsSync(target) && readFileSync(target, 'utf8') !== content) failures.push(`${path} is stale`);
  }
  if (failures.length) fail(failures.join('\n'));
}

function parseArgs(argv) {
  const args = { check: false, updateLock: false, write: false };
  for (const arg of argv) {
    if (arg === '--check') args.check = true;
    else if (arg === '--write') args.write = true;
    else if (arg === '--update-lock') args.updateLock = true;
    else if (arg === '--help' || arg === '-h') args.help = true;
    else fail(`unknown argument: ${arg}`);
  }
  if (args.help) return args;
  if (Number(args.check) + Number(args.write) !== 1) fail('exactly one of --check or --write is required');
  if (args.check && args.updateLock) fail('--update-lock may be used only with --write');
  return args;
}

function main() {
  const args = parseArgs(process.argv.slice(2));
  if (args.help) {
    console.log('Usage: node tools/typespec-protobuf-parity.mjs (--check | --write [--update-lock])');
    return;
  }
  const schema = JSON.parse(readFileSync(SCHEMA_PATH, 'utf8'));
  const model = normalizeSchema(schema);
  const existing = existsSync(FIELD_LOCK_PATH) ? JSON.parse(readFileSync(FIELD_LOCK_PATH, 'utf8')) : null;
  const lock = args.updateLock ? updateFieldNumberLock(model, existing) : validateFieldNumberLock(model, existing);
  if (args.updateLock) writeFileSync(FIELD_LOCK_PATH, stableStringify(lock), 'utf8');
  const { files, manifest } = generateFiles(schema, lock);
  if (args.check) checkGenerated(files, OUTPUT_ROOT);
  else writeGenerated(files, OUTPUT_ROOT);
  console.log(`${args.check ? 'verified' : 'generated'} ${manifest.entityCount} TypeSpec/Protobuf messages and ${manifest.fieldCount} fields`);
}

const isMain = process.argv[1] && resolve(process.argv[1]) === ENTRY_PATH;
if (isMain) {
  try {
    main();
  } catch (error) {
    console.error(error instanceof Error ? error.stack : String(error));
    process.exitCode = 1;
  }
}

export {
  assertSafeFieldNumber,
  generateFiles,
  generateTypeSpec,
  parseProtoMessages,
  projectionForField,
  shapeSha256,
  updateFieldNumberLock,
  validateFieldNumberLock,
  verifyEmittedContracts,
};
