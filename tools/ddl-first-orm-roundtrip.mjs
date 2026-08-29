#!/usr/bin/env node

import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
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
import { dirname, join, relative, resolve, sep } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const repositoryRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const toolingRoot = join(repositoryRoot, 'schema-tooling');
const outputRoot = join(repositoryRoot, 'generated', 'ddl-roundtrip');
const authoredDdlPath = join(repositoryRoot, 'src', 'rust-orm', 'sql', 'registry.sql');
const drizzleConfigPath = join(toolingRoot, 'drizzle.roundtrip.config.ts');
const drizzleKitPath = join(toolingRoot, 'node_modules', '.bin', 'drizzle-kit');
const expectedSeaOrmCliVersion = 'sea-orm-cli 1.1.19';
const guardSqlstates = ['ZD001', 'ZD002', 'ZD003', 'ZD004', 'ZD005'];

function fail(message) {
  throw new Error(message);
}

function countOccurrences(value, needle) {
  return value.split(needle).length - 1;
}

function sha256(value) {
  return createHash('sha256').update(value).digest('hex');
}

function redact(value, secret) {
  return secret ? String(value).split(secret).join('[database-url-redacted]') : String(value);
}

function command(binary, args, options = {}) {
  const result = spawnSync(binary, args, {
    cwd: options.cwd ?? repositoryRoot,
    env: options.env ?? process.env,
    encoding: 'utf8',
    maxBuffer: 64 * 1024 * 1024,
  });
  if (result.error?.code === 'ENOENT') fail(`${options.label ?? binary} is required`);
  if (result.status !== 0) {
    const detail = redact(result.stderr || result.stdout || result.error || 'unknown failure', options.secret).trim();
    fail(`${options.label ?? binary} failed${detail ? `: ${detail}` : ''}`);
  }
  return result.stdout;
}

export function validateDisposableDatabaseUrl(raw) {
  if (!raw) fail('DDL_ROUNDTRIP_DATABASE_URL is required');
  let parsed;
  try {
    parsed = new URL(raw);
  } catch {
    fail('DDL_ROUNDTRIP_DATABASE_URL must be a valid PostgreSQL URL');
  }
  if (!['postgres:', 'postgresql:'].includes(parsed.protocol)) {
    fail('DDL_ROUNDTRIP_DATABASE_URL must use postgres or postgresql');
  }
  if (!['localhost', '127.0.0.1', '[::1]'].includes(parsed.hostname)) {
    fail('DDL round trips may connect only to a loopback PostgreSQL host');
  }
  const database = decodeURIComponent(parsed.pathname.replace(/^\//, ''));
  if (!/^zed_ddl_roundtrip_[a-z0-9_]+$/.test(database)) {
    fail('DDL round-trip database names must start with zed_ddl_roundtrip_');
  }
  return { parsed, database };
}

function findClosingBracket(source, openingIndex) {
  let depth = 0;
  let quote = null;
  let escaped = false;
  for (let index = openingIndex; index < source.length; index += 1) {
    const character = source[index];
    if (quote) {
      if (escaped) escaped = false;
      else if (character === '\\') escaped = true;
      else if (character === quote) quote = null;
      continue;
    }
    if (character === "'" || character === '"' || character === '`') {
      quote = character;
      continue;
    }
    if (character === '[') depth += 1;
    else if (character === ']') {
      depth -= 1;
      if (depth === 0) return index;
    }
  }
  fail('Drizzle table metadata array is unterminated');
}

function findClosingParenthesis(source, openingIndex) {
  let depth = 0;
  let quote = null;
  let escaped = false;
  let lineComment = false;
  let blockComment = false;
  for (let index = openingIndex; index < source.length; index += 1) {
    const character = source[index];
    const next = source[index + 1];
    if (lineComment) {
      if (character === '\n') lineComment = false;
      continue;
    }
    if (blockComment) {
      if (character === '*' && next === '/') {
        blockComment = false;
        index += 1;
      }
      continue;
    }
    if (quote) {
      if (escaped) escaped = false;
      else if (character === '\\') escaped = true;
      else if (character === quote) quote = null;
      continue;
    }
    if (character === '/' && next === '/') {
      lineComment = true;
      index += 1;
      continue;
    }
    if (character === '/' && next === '*') {
      blockComment = true;
      index += 1;
      continue;
    }
    if (character === "'" || character === '"' || character === '`') {
      quote = character;
      continue;
    }
    if (character === '(') depth += 1;
    else if (character === ')') {
      depth -= 1;
      if (depth === 0) return index;
    }
  }
  fail('Drizzle table declaration is unterminated');
}

function splitTopLevelEntries(source) {
  const entries = [];
  let start = 0;
  let parentheses = 0;
  let braces = 0;
  let brackets = 0;
  let quote = null;
  let escaped = false;
  for (let index = 0; index < source.length; index += 1) {
    const character = source[index];
    if (quote) {
      if (escaped) escaped = false;
      else if (character === '\\') escaped = true;
      else if (character === quote) quote = null;
      continue;
    }
    if (character === "'" || character === '"' || character === '`') {
      quote = character;
      continue;
    }
    if (character === '(') parentheses += 1;
    else if (character === ')') parentheses -= 1;
    else if (character === '{') braces += 1;
    else if (character === '}') braces -= 1;
    else if (character === '[') brackets += 1;
    else if (character === ']') brackets -= 1;
    else if (character === ',' && parentheses === 0 && braces === 0 && brackets === 0) {
      const entry = source.slice(start, index).trim();
      if (entry) entries.push(entry);
      start = index + 1;
    }
    if (parentheses < 0 || braces < 0 || brackets < 0) fail('Drizzle metadata has unbalanced delimiters');
  }
  const tail = source.slice(start).trim();
  if (tail) entries.push(tail);
  if (parentheses !== 0 || braces !== 0 || brackets !== 0 || quote) {
    fail('Drizzle metadata has unbalanced delimiters or quotes');
  }
  return entries;
}

function metadataName(entry) {
  const direct = entry.match(/^(?:index|uniqueIndex|check)\("([^"]+)"/s)?.[1];
  const foreignKey = entry.match(/^foreignKey\(\{[\s\S]*?\bname:\s*"([^"]+)"/s)?.[1];
  const primaryKey = entry.match(/^primaryKey\(\{[\s\S]*?\bname:\s*"([^"]+)"/s)?.[1];
  const name = direct ?? foreignKey ?? primaryKey;
  if (!name) fail(`cannot identify Drizzle metadata entry: ${entry.slice(0, 80)}`);
  return name;
}

export function canonicalizeDrizzleTableMetadata(source) {
  const marker = '(table) => [';
  let cursor = 0;
  let canonical = '';
  let tableCount = 0;
  while (true) {
    const markerIndex = source.indexOf(marker, cursor);
    if (markerIndex < 0) break;
    const openingIndex = markerIndex + marker.length - 1;
    const closingIndex = findClosingBracket(source, openingIndex);
    const entries = splitTopLevelEntries(source.slice(openingIndex + 1, closingIndex));
    const keyed = entries.map((entry) => [metadataName(entry), entry]);
    const unique = new Set(keyed.map(([name]) => name));
    if (unique.size !== keyed.length) fail('Drizzle table metadata contains duplicate names');
    keyed.sort(([left], [right]) => left.localeCompare(right));
    const body = keyed.length ? `\n${keyed.map(([, entry]) => `\t${entry}`).join(',\n')},\n` : '';
    canonical += source.slice(cursor, openingIndex + 1) + body + ']';
    cursor = closingIndex + 1;
    tableCount += 1;
  }
  canonical += source.slice(cursor);
  if (tableCount && tableCount !== 17) fail(`expected 17 Drizzle table metadata arrays, found ${tableCount}`);
  return canonical;
}

export function stripDrizzleImplicitOperatorClasses(source) {
  const operatorCallCount = countOccurrences(source, '.op(');
  const operatorPattern = /\.op\("[a-z0-9_]+_ops"\)/g;
  const recognized = [...source.matchAll(operatorPattern)].length;
  if (operatorCallCount !== recognized) {
    fail(`Drizzle emitted an unrecognized operator-class expression: ${operatorCallCount} calls, ${recognized} implicit classes`);
  }
  const stripped = source.replace(operatorPattern, '');
  if (stripped.includes('.op(')) fail('Drizzle schema still contains an operator-class expression');
  return stripped;
}

export function canonicalizeDrizzleTableOrder(source) {
  const declarationPattern = /^export const (zed_[a-z0-9_]+) = pgTable\(/gm;
  const blocks = [];
  let match;
  while ((match = declarationPattern.exec(source)) !== null) {
    const openingIndex = match.index + match[0].length - 1;
    const closingIndex = findClosingParenthesis(source, openingIndex);
    if (source[closingIndex + 1] !== ';') fail(`Drizzle table ${match[1]} is not terminated by a semicolon`);
    blocks.push({
      name: match[1],
      start: match.index,
      end: closingIndex + 2,
      source: source.slice(match.index, closingIndex + 2),
    });
  }
  if (blocks.length < 2) return source;
  const names = new Set(blocks.map(({ name }) => name));
  if (names.size !== blocks.length) fail('Drizzle schema contains duplicate table declarations');
  for (let index = 1; index < blocks.length; index += 1) {
    const gap = source.slice(blocks[index - 1].end, blocks[index].start);
    if (gap.trim()) fail('Drizzle schema contains unsupported content between table declarations');
  }
  const prefix = source.slice(0, blocks[0].start);
  const suffix = source.slice(blocks.at(-1).end);
  const ordered = [...blocks].sort(({ name: left }, { name: right }) => left.localeCompare(right));
  return `${prefix}${ordered.map(({ source: block }) => block).join('\n\n')}${suffix}`;
}

export function normalizePulledDrizzleSchema(source) {
  const knownBrokenLine = "\trepo_url: text().default(').notNull(),";
  const brokenDefault = ".default(').notNull()";
  const knownCount = countOccurrences(source, knownBrokenLine);
  const totalCount = countOccurrences(source, brokenDefault);
  if (knownCount !== 1 || totalCount !== 1) {
    fail(`Drizzle empty-string normalization expected one repo_url repair, found ${knownCount} known and ${totalCount} total`);
  }
  const repaired = source.replace(knownBrokenLine, "\trepo_url: text().default('').notNull(),");
  if (repaired.includes(brokenDefault)) fail('Drizzle schema still contains an unterminated empty-string default');
  const withoutOperatorClasses = stripDrizzleImplicitOperatorClasses(repaired);
  const canonicalMetadata = canonicalizeDrizzleTableMetadata(withoutOperatorClasses);
  const canonical = canonicalizeDrizzleTableOrder(canonicalMetadata);
  return `// SHADOW ONLY. Generated from authored DDL; never use this file as migration authority.\n${canonical}`;
}

export function assertAuthoredDdlSemantics(sql) {
  const lower = sql.toLowerCase();
  if (/\b[a-z][a-z0-9_]*_ops\b/i.test(sql)) {
    fail('authored DDL uses an explicit operator class that the Drizzle shadow canonicalizer must preserve');
  }
  const tableCount = [...lower.matchAll(/create\s+table\s+if\s+not\s+exists\s+zed_[a-z0-9_]+\s*\(/g)].length;
  if (tableCount !== 17) fail(`authored DDL must define 17 Zed tables, found ${tableCount}`);
  for (const code of guardSqlstates) {
    if (!sql.includes(code)) fail(`authored DDL is missing guard SQLSTATE ${code}`);
  }
  for (const fragment of ['create or replace function', 'create trigger']) {
    if (!lower.includes(fragment)) fail(`authored DDL is missing ${fragment}`);
  }
  return tableCount;
}

export function assertGeneratedSqlIsNonAuthoritative(sql) {
  const lower = sql.toLowerCase();
  const tableCount = [...lower.matchAll(/create\s+table\s+"zed_[a-z0-9_]+"\s*\(/g)].length;
  if (tableCount !== 17) fail(`Drizzle export must project 17 Zed tables, found ${tableCount}`);
  for (const forbidden of ['create function', 'create trigger', ...guardSqlstates.map((code) => code.toLowerCase())]) {
    if (lower.includes(forbidden)) fail(`Drizzle export unexpectedly contains authored-only semantic ${forbidden}`);
  }
}

function psql(databaseUrl, sql) {
  return command(process.env.PSQL ?? 'psql', [
    '--no-psqlrc',
    '--tuples-only',
    '--no-align',
    '--set',
    'ON_ERROR_STOP=1',
    '--dbname',
    databaseUrl,
    '--command',
    sql,
  ], { label: 'psql', secret: databaseUrl }).trim();
}

function assertEmptyPublicSchema(databaseUrl) {
  const count = psql(databaseUrl, `
    SELECT count(*)
      FROM pg_catalog.pg_class AS relation
      JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = relation.relnamespace
     WHERE namespace.nspname = 'public'
       AND relation.relkind IN ('r', 'p');
  `);
  if (count !== '0') fail(`disposable database public schema must be empty, found ${count} tables`);
}

function applyAuthoredDdl(databaseUrl) {
  command(process.env.PSQL ?? 'psql', [
    '--no-psqlrc',
    '--set',
    'ON_ERROR_STOP=1',
    '--dbname',
    databaseUrl,
    '--file',
    authoredDdlPath,
  ], { label: 'authored DDL application', secret: databaseUrl });
}

function databaseProfile(databaseUrl) {
  const rows = psql(databaseUrl, `
    SELECT 'checks', count(*) FROM pg_constraint AS c
      JOIN pg_class AS relation ON relation.oid = c.conrelid
      JOIN pg_namespace AS namespace ON namespace.oid = relation.relnamespace
      WHERE namespace.nspname = 'public' AND relation.relname LIKE 'zed\\_%' ESCAPE '\\' AND c.contype = 'c'
    UNION ALL SELECT 'columns', count(*) FROM information_schema.columns
      WHERE table_schema = 'public' AND table_name LIKE 'zed\\_%' ESCAPE '\\'
    UNION ALL SELECT 'foreignKeys', count(*) FROM pg_constraint AS c
      JOIN pg_class AS relation ON relation.oid = c.conrelid
      JOIN pg_namespace AS namespace ON namespace.oid = relation.relnamespace
      WHERE namespace.nspname = 'public' AND relation.relname LIKE 'zed\\_%' ESCAPE '\\' AND c.contype = 'f'
    UNION ALL SELECT 'functions', count(*) FROM pg_proc AS p
      JOIN pg_namespace AS namespace ON namespace.oid = p.pronamespace
      WHERE namespace.nspname = 'public' AND p.proname LIKE 'zed\\_%' ESCAPE '\\'
    UNION ALL SELECT 'policies', count(*) FROM pg_policies
      WHERE schemaname = 'public' AND tablename LIKE 'zed\\_%' ESCAPE '\\'
    UNION ALL SELECT 'secondaryIndexes', count(*) FROM pg_index AS i
      JOIN pg_class AS relation ON relation.oid = i.indrelid
      JOIN pg_namespace AS namespace ON namespace.oid = relation.relnamespace
      WHERE namespace.nspname = 'public' AND relation.relname LIKE 'zed\\_%' ESCAPE '\\'
        AND NOT i.indisprimary
        AND NOT EXISTS (SELECT 1 FROM pg_constraint AS c WHERE c.conindid = i.indexrelid)
    UNION ALL SELECT 'tables', count(*) FROM pg_class AS relation
      JOIN pg_namespace AS namespace ON namespace.oid = relation.relnamespace
      WHERE namespace.nspname = 'public' AND relation.relkind IN ('r', 'p') AND relation.relname LIKE 'zed\\_%' ESCAPE '\\'
    UNION ALL SELECT 'triggers', count(*) FROM pg_trigger AS t
      JOIN pg_class AS relation ON relation.oid = t.tgrelid
      JOIN pg_namespace AS namespace ON namespace.oid = relation.relnamespace
      WHERE namespace.nspname = 'public' AND relation.relname LIKE 'zed\\_%' ESCAPE '\\' AND NOT t.tgisinternal
    ORDER BY 1;
  `);
  const profile = Object.fromEntries(rows.split('\n').filter(Boolean).map((row) => {
    const [name, count] = row.split('|');
    if (!name || !/^\d+$/.test(count)) fail(`invalid database profile row: ${row}`);
    return [name, Number(count)];
  }));
  if (profile.tables !== 17) fail(`applied DDL produced ${profile.tables} tables instead of 17`);
  if (profile.columns !== 213 || profile.foreignKeys !== 40 || profile.checks !== 109) {
    fail(`applied DDL shape changed unexpectedly: ${JSON.stringify(profile)}`);
  }

  const functionDefinitions = psql(databaseUrl, `
    SELECT coalesce(string_agg(pg_get_functiondef(p.oid), E'\\n'), '')
      FROM pg_proc AS p
      JOIN pg_namespace AS namespace ON namespace.oid = p.pronamespace
     WHERE namespace.nspname = 'public'
       AND p.proname LIKE 'zed\\_%' ESCAPE '\\';
  `);
  for (const code of guardSqlstates) {
    if (!functionDefinitions.includes(code)) fail(`live disposable schema lost guard SQLSTATE ${code}`);
  }
  return profile;
}

function listFiles(root) {
  if (!existsSync(root)) return [];
  const output = [];
  const visit = (directory) => {
    for (const entry of readdirSync(directory).sort()) {
      const absolute = join(directory, entry);
      if (statSync(absolute).isDirectory()) visit(absolute);
      else output.push(relative(root, absolute).split(sep).join('/'));
    }
  };
  visit(root);
  return output;
}

function writeOrCheck(files, mode) {
  const expectedPaths = [...files.keys()].sort();
  if (mode === 'write') {
    if (outputRoot !== join(repositoryRoot, 'generated', 'ddl-roundtrip')) {
      fail('refusing to replace an unexpected generated output root');
    }
    rmSync(outputRoot, { recursive: true, force: true });
    for (const [path, content] of files) {
      const destination = join(outputRoot, path);
      mkdirSync(dirname(destination), { recursive: true });
      writeFileSync(destination, content);
    }
    return;
  }
  if (!existsSync(outputRoot)) fail('generated/ddl-roundtrip is missing; run the --write command in a disposable database');
  const actualPaths = listFiles(outputRoot);
  if (JSON.stringify(actualPaths) !== JSON.stringify(expectedPaths)) {
    fail(`generated file set differs\nexpected: ${expectedPaths.join(', ')}\nactual: ${actualPaths.join(', ')}`);
  }
  for (const [path, expected] of files) {
    const actual = readFileSync(join(outputRoot, path), 'utf8');
    if (actual !== expected) {
      const actualLines = actual.split('\n');
      const expectedLines = expected.split('\n');
      const line = expectedLines.findIndex((value, index) => value !== actualLines[index]) + 1;
      fail(`generated DDL round-trip artifact drifted: ${path} at line ${line} (committed ${sha256(actual)}, generated ${sha256(expected)})`);
    }
  }
}

function buildArtifacts(workRoot, databaseUrl, authoredDdl, profile, seaOrmCli) {
  const drizzleOutput = join(workRoot, 'drizzle');
  const seaOrmOutput = join(workRoot, 'sea-orm');
  mkdirSync(seaOrmOutput, { recursive: true });

  const drizzleEnvironment = {
    ...process.env,
    DDL_ROUNDTRIP_DATABASE_URL: databaseUrl,
    DDL_ROUNDTRIP_DRIZZLE_OUT: drizzleOutput,
  };
  command(drizzleKitPath, ['pull', '--config', drizzleConfigPath], {
    cwd: toolingRoot,
    env: drizzleEnvironment,
    label: 'drizzle-kit pull',
    secret: databaseUrl,
  });
  const drizzleSchema = normalizePulledDrizzleSchema(readFileSync(join(drizzleOutput, 'schema.ts'), 'utf8'));
  writeFileSync(join(drizzleOutput, 'schema.ts'), drizzleSchema);
  const exportedSql = command(drizzleKitPath, [
    'export',
    '--dialect',
    'postgresql',
    '--schema',
    join(drizzleOutput, 'schema.ts'),
  ], { cwd: toolingRoot, label: 'drizzle-kit export' });
  assertGeneratedSqlIsNonAuthoritative(exportedSql);

  command(seaOrmCli, [
    'generate',
    'entity',
    '--database-url',
    databaseUrl,
    '--database-schema',
    'public',
    '--output-dir',
    seaOrmOutput,
    '--compact-format',
    '--with-prelude',
    'none',
    '--impl-active-model-behavior=true',
  ], { label: 'sea-orm-cli entity generation', secret: databaseUrl });

  const files = new Map([
    ['README.md', '# DDL round-trip shadow artifacts\n\nThese files are generated from `src/rust-orm/sql/registry.sql` through a disposable PostgreSQL database. They prove ORM projection parity. They are **not migrations** and must never be applied to a database.\n'],
    ['drizzle/schema.ts', drizzleSchema],
    ['drizzle/schema.sql', `-- SHADOW ONLY. Generated by Drizzle from its introspected model.\n-- Functions, triggers, guard SQLSTATEs, grants, ownership, and other authored semantics are intentionally absent.\n\n${exportedSql}`],
  ]);
  for (const path of listFiles(seaOrmOutput)) {
    const content = readFileSync(join(seaOrmOutput, path), 'utf8');
    files.set(`sea-orm/${path}`, `//! SHADOW ONLY: generated from the authored DDL for parity certification.\n${content}`);
  }
  const manifest = {
    schemaVersion: 1,
    authority: {
      repository: 'zed-pkg/zed-lib-core',
      package: 'zed-pkg/zed-schema',
      packageManifest: 'src/rust-orm/sql/.zpkg.toml',
      ddl: 'src/rust-orm/sql/registry.sql',
      ddlSha256: sha256(authoredDdl),
      generatedSqlMayBeApplied: false,
    },
    generators: {
      drizzleKit: command(drizzleKitPath, ['--version'], { cwd: toolingRoot, label: 'drizzle-kit version' }).trim(),
      seaOrmCli: expectedSeaOrmCliVersion,
    },
    databaseProfile: profile,
    guardSqlstates,
    authoredOnlySemantics: [
      'functions',
      'triggers',
      'guard SQLSTATEs',
      'grants and ownership',
      'row-level security policies when present',
      'migration ledger identity',
    ],
    artifacts: [...files.keys()].sort(),
  };
  files.set('manifest.json', `${JSON.stringify(manifest, null, 2)}\n`);
  return files;
}

export function parseMode(args) {
  if (args.length !== 1 || !['--check', '--write'].includes(args[0])) {
    fail('usage: node tools/ddl-first-orm-roundtrip.mjs --check|--write');
  }
  return args[0].slice(2);
}

export function main(args = process.argv.slice(2)) {
  const mode = parseMode(args);
  if (process.env.DDL_ROUNDTRIP_ALLOW_WRITE !== '1') {
    fail('DDL_ROUNDTRIP_ALLOW_WRITE=1 is required because this command applies DDL');
  }
  const databaseUrl = process.env.DDL_ROUNDTRIP_DATABASE_URL;
  validateDisposableDatabaseUrl(databaseUrl);
  if (!existsSync(drizzleKitPath)) fail('run npm ci in schema-tooling before the DDL round trip');
  const seaOrmCli = process.env.SEA_ORM_CLI ?? 'sea-orm-cli';
  const seaVersion = command(seaOrmCli, ['--version'], { label: 'sea-orm-cli version' }).trim();
  if (seaVersion !== expectedSeaOrmCliVersion) {
    fail(`sea-orm-cli must be exactly 1.1.19, found ${seaVersion || 'unknown'}`);
  }
  const authoredDdl = readFileSync(authoredDdlPath, 'utf8');
  assertAuthoredDdlSemantics(authoredDdl);
  assertEmptyPublicSchema(databaseUrl);
  applyAuthoredDdl(databaseUrl);
  const profile = databaseProfile(databaseUrl);

  const workRoot = mkdtempSync(join(toolingRoot, '.ddl-roundtrip-work-'));
  try {
    const files = buildArtifacts(workRoot, databaseUrl, authoredDdl, profile, seaOrmCli);
    writeOrCheck(files, mode);
    process.stdout.write(`${mode === 'write' ? 'wrote' : 'verified'} ${files.size} DDL round-trip artifacts from the authored schema\n`);
  } finally {
    rmSync(workRoot, { recursive: true, force: true });
  }
}

const invokedPath = process.argv[1] ? pathToFileURL(resolve(process.argv[1])).href : '';
if (import.meta.url === invokedPath) main();
