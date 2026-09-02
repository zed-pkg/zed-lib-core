#!/usr/bin/env python3
"""Build the final public-intake persistence tree from current main plus staged source."""

from __future__ import annotations

import hashlib
import re
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
INTERFACES_REV = "ed9b3b67fe24741dd96db0490e80d95cf37d1a4f"
BASELINE_FILES = [
    "Cargo.lock",
    "src/rust-orm/Cargo.toml",
    "src/rust-orm/lib.rs",
    "src/rust-orm/write.rs",
    "src/rust-orm/migrations.rs",
    "src/rust-orm/schema.rs",
    "src/rust-orm/sql/registry.sql",
    "src/rust-orm/tests/public_surface.rs",
]


def git(*args: str, text: bool = True) -> str:
    return subprocess.check_output(["git", *args], cwd=ROOT, text=text).strip()


def baseline(path: str) -> None:
    target = ROOT / path
    target.parent.mkdir(parents=True, exist_ok=True)
    data = subprocess.check_output(["git", "show", f"origin/main:{path}"], cwd=ROOT)
    target.write_bytes(data)


def insert_dependency(text: str, section: str, key: str, value: str) -> str:
    heading = re.search(rf"(?m)^\[{re.escape(section)}\]\s*$", text)
    line = f"{key} = {value}"
    if heading is None:
        return text.rstrip() + f"\n\n[{section}]\n{line}\n"
    next_heading = re.search(r"(?m)^\[[^\n]+\]\s*$", text[heading.end() :])
    end = heading.end() + (next_heading.start() if next_heading else len(text) - heading.end())
    body = text[heading.end() : end]
    pattern = re.compile(rf"(?m)^{re.escape(key)}\s*=.*$")
    if pattern.search(body):
        body = pattern.sub(line, body, count=1)
    else:
        body = body.rstrip() + "\n" + line + "\n"
    return text[: heading.end()] + body + text[end:]


def replace_once(text: str, old: str, new: str, label: str) -> str:
    count = text.count(old)
    if count != 1:
        raise RuntimeError(f"expected exactly one {label} anchor; found {count}")
    return text.replace(old, new, 1)


def add_module_and_exports() -> None:
    path = ROOT / "src/rust-orm/lib.rs"
    text = path.read_text()
    text = replace_once(
        text,
        '#[cfg(feature = "read-write")]\npub mod write;\n',
        '#[cfg(feature = "read-write")]\npub mod write;\n#[cfg(feature = "read-write")]\npub mod public_intake;\n',
        "read-write module",
    )
    text = replace_once(
        text,
        'pub use migrations::{\n    MigrationOutcome, RegistryMigrationStatus, registry_version, run_registry_migrations,\n};\n',
        'pub use migrations::{\n    MigrationOutcome, RegistryMigrationStatus, public_intake_version, registry_version,\n    run_registry_migrations,\n};\n#[cfg(feature = "read-write")]\npub use public_intake::{\n    NewPublicIntakeSubmission, PublicIntakeInsertResult, PublicIntakeStoreError,\n    PublicIntakeSubmissionKind, SecretBytes,\n};\n',
        "migration exports",
    )
    text = replace_once(
        text,
        "    DEPENDENCY_GRAPH_MIGRATION_PATH, ORG_SCHEMA, REGISTRY_DDL_BLOB_SHA, REGISTRY_DDL_PATH,\n",
        "    DEPENDENCY_GRAPH_MIGRATION_PATH, ORG_SCHEMA, PUBLIC_INTAKE_MIGRATION_BLOB_SHA,\n    PUBLIC_INTAKE_MIGRATION_IDENTITY_SUFFIX, PUBLIC_INTAKE_MIGRATION_PATH,\n    REGISTRY_DDL_BLOB_SHA, REGISTRY_DDL_PATH,\n",
        "schema exports",
    )
    path.write_text(text)


def add_write_context_method() -> None:
    path = ROOT / "src/rust-orm/write.rs"
    text = path.read_text()
    structure = re.search(r"pub struct WriteContext\s*\{(?P<body>[\s\S]*?)\n\}", text)
    if structure:
        field = re.search(
            r"(?m)^\s*(?:pub(?:\([^)]*\))?\s+)?([A-Za-z_][A-Za-z0-9_]*)\s*:\s*(?:std::sync::Arc<|Arc<)?DatabaseConnection>?\s*,?\s*$",
            structure.group("body"),
        )
        if field is None:
            raise RuntimeError("cannot locate WriteContext database field")
        database = f"&self.{field.group(1)}"
    elif re.search(r"pub struct WriteContext\s*\(\s*DatabaseConnection\s*\)\s*;", text):
        database = "&self.0"
    else:
        raise RuntimeError("cannot locate WriteContext representation")
    text += f'''\nimpl WriteContext {{
    /// Persist a validated commercial-intake request without exposing the
    /// underlying connection or generated persistence entity to route code.
    pub async fn insert_public_intake_submission(
        &self,
        input: crate::public_intake::NewPublicIntakeSubmission,
    ) -> Result<
        crate::public_intake::PublicIntakeInsertResult,
        crate::public_intake::PublicIntakeStoreError,
    > {{
        crate::public_intake::insert_public_intake_submission_on_database(
            {database},
            input,
        )
        .await
    }}
}}
'''
    path.write_text(text)


def add_migration() -> None:
    migration_path = ROOT / "src/rust-orm/sql/2026-09-02-public-intake.sql"
    migration_sql = migration_path.read_text().strip()
    registry_path = ROOT / "src/rust-orm/sql/registry.sql"
    registry = registry_path.read_text().rstrip()
    registry += "\n\n" + migration_sql + "\n"
    if registry.lower().count("create table if not exists zed_public_intake_submissions") != 1:
        raise RuntimeError("public-intake desired-state table must occur exactly once")
    registry_path.write_text(registry)

    migration_blob = git("hash-object", str(migration_path.relative_to(ROOT)))
    registry_blob = git("hash-object", str(registry_path.relative_to(ROOT)))

    schema_path = ROOT / "src/rust-orm/schema.rs"
    schema = schema_path.read_text()
    schema, count = re.subn(
        r'pub const REGISTRY_DDL_BLOB_SHA: &str = "[0-9a-f]{40}";',
        f'pub const REGISTRY_DDL_BLOB_SHA: &str = "{registry_blob}";',
        schema,
        count=1,
    )
    if count != 1:
        raise RuntimeError("registry DDL blob constant not found")
    constants = f'''/// Package-local forward migration for encrypted public commercial intake.
pub const PUBLIC_INTAKE_MIGRATION_PATH: &str =
    "src/rust-orm/sql/2026-09-02-public-intake.sql";

/// Immutable Git blob identity of the public-intake forward migration.
pub const PUBLIC_INTAKE_MIGRATION_BLOB_SHA: &str = "{migration_blob}";

/// Ledger suffix for the first public-intake migration.
pub const PUBLIC_INTAKE_MIGRATION_IDENTITY_SUFFIX: &str =
    PUBLIC_INTAKE_MIGRATION_BLOB_SHA;

'''
    schema = replace_once(
        schema,
        "// Compatibility aliases retained for existing Rust consumers.",
        constants + "// Compatibility aliases retained for existing Rust consumers.",
        "schema constant",
    )
    schema_path.write_text(schema)

    path = ROOT / "src/rust-orm/migrations.rs"
    text = path.read_text()
    text = replace_once(
        text,
        "    SHARED_DEFS_REGISTRY_SEGMENT, VISIBILITY_IMMUTABILITY_MIGRATION_IDENTITY_SUFFIX,\n",
        "    PUBLIC_INTAKE_MIGRATION_IDENTITY_SUFFIX, SHARED_DEFS_REGISTRY_SEGMENT,\n    VISIBILITY_IMMUTABILITY_MIGRATION_IDENTITY_SUFFIX,\n",
        "migration import",
    )
    text = replace_once(
        text,
        'const VISIBILITY_IMMUTABILITY_SQL: &str =\n    include_str!("sql/2026-08-11-public-visibility-is-permanent.sql");\n',
        'const VISIBILITY_IMMUTABILITY_SQL: &str =\n    include_str!("sql/2026-08-11-public-visibility-is-permanent.sql");\nconst PUBLIC_INTAKE_SQL: &str = include_str!("sql/2026-09-02-public-intake.sql");\n',
        "migration SQL include",
    )
    text = replace_once(
        text,
        "/// Return the current registry migration identity.\n",
        '''/// Return the package-owned public-intake migration identity.
pub fn public_intake_version() -> String {
    format!("registry-public-intake@{PUBLIC_INTAKE_MIGRATION_IDENTITY_SUFFIX}")
}

/// Return the current registry migration identity.
''',
        "migration version function",
    )
    text = replace_once(
        text,
        "    visibility_immutability_version()\n",
        "    public_intake_version()\n",
        "registry version",
    )
    text = replace_once(
        text,
        "    VisibilityImmutability,\n",
        "    VisibilityImmutability,\n    PublicIntake,\n",
        "migration enum",
    )
    ordered = re.search(r"const ORDERED: \[Self; (?P<count>\d+)\] = \[(?P<body>[\s\S]*?)\n\s*\];", text)
    if ordered is None:
        raise RuntimeError("ordered migration list not found")
    body = ordered.group("body") + "\n        Self::PublicIntake,"
    replacement = f"const ORDERED: [Self; {int(ordered.group('count')) + 1}] = [" + body + "\n    ];"
    text = text[: ordered.start()] + replacement + text[ordered.end() :]
    text = replace_once(
        text,
        "            Self::VisibilityImmutability => visibility_immutability_version(),\n",
        "            Self::VisibilityImmutability => visibility_immutability_version(),\n            Self::PublicIntake => public_intake_version(),\n",
        "migration version dispatch",
    )
    text = replace_once(
        text,
        "            Self::VisibilityImmutability => VISIBILITY_IMMUTABILITY_SQL,\n",
        "            Self::VisibilityImmutability => VISIBILITY_IMMUTABILITY_SQL,\n            Self::PublicIntake => PUBLIC_INTAKE_SQL,\n",
        "migration SQL dispatch",
    )
    # Update the repository's three migration-order/identity test fixtures without
    # weakening any assertion.
    text = text.replace(
        "                RegistryMigration::VisibilityImmutability,\n            ]",
        "                RegistryMigration::VisibilityImmutability,\n                RegistryMigration::PublicIntake,\n            ]",
    )
    text = text.replace(
        "        assert_eq!(registry_version(), visibility);",
        "        let intake = public_intake_version();\n        assert!(intake.ends_with(PUBLIC_INTAKE_MIGRATION_IDENTITY_SUFFIX));\n        assert_ne!(visibility, intake);\n        assert_eq!(registry_version(), intake);",
    )
    text = text.replace(
        "            visibility_immutability_version(),\n        ];",
        "            visibility_immutability_version(),\n            public_intake_version(),\n        ];",
    )
    path.write_text(text)


def main() -> None:
    for path in BASELINE_FILES:
        baseline(path)

    source = ROOT / ".github/materialize/public_intake.rs"
    sql = ROOT / ".github/materialize/2026-09-02-public-intake.sql"
    if not source.is_file() or not sql.is_file():
        raise RuntimeError("staged public-intake source is missing")
    (ROOT / "src/rust-orm/public_intake.rs").write_bytes(source.read_bytes())
    (ROOT / "src/rust-orm/sql/2026-09-02-public-intake.sql").write_bytes(sql.read_bytes())

    cargo_path = ROOT / "src/rust-orm/Cargo.toml"
    cargo = cargo_path.read_text()
    cargo, count = re.subn(
        r'zed-interfaces\s*=\s*\{\s*git\s*=\s*"https://github\.com/zed-pkg/zed-interfaces\.git",\s*rev\s*=\s*"[0-9a-f]{40}"\s*\}',
        f'zed-interfaces = {{ git = "https://github.com/zed-pkg/zed-interfaces.git", rev = "{INTERFACES_REV}" }}',
        cargo,
        count=1,
    )
    if count != 1:
        raise RuntimeError("zed-interfaces dependency not found")
    for key, value in [
        ("aes-gcm", '"0.10"'),
        ("hex", '"0.4"'),
        ("hmac", '"0.12"'),
        ("rand", '"0.9"'),
        ("sha2", '"0.10"'),
        ("zeroize", '"1"'),
    ]:
        cargo = insert_dependency(cargo, "dependencies", key, value)
    cargo = insert_dependency(
        cargo,
        "dev-dependencies",
        "tokio",
        '{ version = "1", features = ["macros", "rt-multi-thread"] }',
    )
    cargo_path.write_text(cargo)

    surface_path = ROOT / "src/rust-orm/tests/public_surface.rs"
    surface = surface_path.read_text()
    surface, count = re.subn(
        r'pub const ZED_INTERFACES_GIT_REV: &str = "[0-9a-f]{40}";',
        f'pub const ZED_INTERFACES_GIT_REV: &str = "{INTERFACES_REV}";',
        surface,
        count=1,
    )
    if count != 1:
        raise RuntimeError("public-surface interface revision not found")
    surface_path.write_text(surface)

    add_module_and_exports()
    add_write_context_method()
    add_migration()

    # The generated directories are authoritative output; remove branch-only
    # attempts before the canonical generators recreate them.
    subprocess.run(["git", "checkout", "origin/main", "--", "generated"], cwd=ROOT, check=True)


if __name__ == "__main__":
    main()
