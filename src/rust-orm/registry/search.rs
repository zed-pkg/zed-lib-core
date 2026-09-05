use sea_orm::{prelude::Uuid, ConnectionTrait, Statement, Value};
use serde_json::{Number, Value as Json};

#[cfg(feature = "read-write")]
use crate::WriteContext;
use crate::{OrmError, ReadContext};

use super::validation::{embedding, embedding_model};
#[cfg(feature = "read-write")]
use super::validation::{entity_type, optional_text, sha256};

const TEXT_SEARCH_SQL: &str = r#"
WITH visible_orgs AS (
    SELECT value::uuid AS org_id
    FROM jsonb_array_elements_text($2::jsonb)
), hits AS (
    SELECT
        'org'::text AS entity_type,
        o.id AS entity_id,
        o.slug AS label,
        o.description,
        CASE
            WHEN lower(o.slug) = lower($1) THEN 1.0
            WHEN o.slug ILIKE $1 || '%' OR o.name ILIKE $1 || '%' THEN 0.8
            WHEN o.slug ILIKE '%' || $1 || '%' OR o.name ILIKE '%' || $1 || '%' THEN 0.6
            ELSE 0.3
        END::double precision AS score
    FROM zed_orgs o
    WHERE o.id IN (SELECT org_id FROM visible_orgs)
      AND o.is_soft_deleted = false
      AND (
          o.slug ILIKE '%' || $1 || '%'
          OR o.name ILIKE '%' || $1 || '%'
          OR coalesce(o.description, '') ILIKE '%' || $1 || '%'
      )

    UNION ALL

    SELECT
        'project'::text,
        p.id,
        o.slug || '/' || p.slug,
        p.description,
        CASE
            WHEN lower(p.slug) = lower($1) THEN 1.0
            WHEN p.slug ILIKE $1 || '%' OR p.name ILIKE $1 || '%' THEN 0.8
            WHEN p.slug ILIKE '%' || $1 || '%' OR p.name ILIKE '%' || $1 || '%' THEN 0.6
            ELSE 0.3
        END::double precision
    FROM zed_projects p
    JOIN zed_orgs o ON o.id = p.org_id
    WHERE p.org_id IN (SELECT org_id FROM visible_orgs)
      AND p.is_soft_deleted = false
      AND o.is_soft_deleted = false
      AND (
          p.slug ILIKE '%' || $1 || '%'
          OR p.name ILIKE '%' || $1 || '%'
          OR coalesce(p.description, '') ILIKE '%' || $1 || '%'
      )

    UNION ALL

    SELECT
        'package'::text,
        p.id,
        o.slug || '/' || p.name,
        p.description,
        CASE
            WHEN lower(p.name) = lower($1) THEN 1.0
            WHEN p.name ILIKE $1 || '%' THEN 0.8
            WHEN p.name ILIKE '%' || $1 || '%' OR p.repo_url ILIKE '%' || $1 || '%' THEN 0.6
            ELSE 0.3
        END::double precision
    FROM zed_packages p
    JOIN zed_orgs o ON o.id = p.org_id
    WHERE (p.visibility = 'public' OR p.org_id IN (SELECT org_id FROM visible_orgs))
      AND p.is_soft_deleted = false
      AND o.is_soft_deleted = false
      AND (
          p.name ILIKE '%' || $1 || '%'
          OR p.repo_url ILIKE '%' || $1 || '%'
          OR coalesce(p.description, '') ILIKE '%' || $1 || '%'
      )

    UNION ALL

    SELECT
        'package_version'::text,
        v.id,
        o.slug || '/' || p.name || '@' || v.version,
        p.description,
        CASE
            WHEN lower(v.version) = lower($1) THEN 1.0
            WHEN v.version ILIKE $1 || '%' THEN 0.8
            ELSE 0.5
        END::double precision
    FROM zed_package_versions v
    JOIN zed_packages p ON p.id = v.package_id
    JOIN zed_orgs o ON o.id = p.org_id
    WHERE (p.visibility = 'public' OR p.org_id IN (SELECT org_id FROM visible_orgs))
      AND p.is_soft_deleted = false
      AND o.is_soft_deleted = false
      AND (v.version ILIKE '%' || $1 || '%' OR v.vcs_tag ILIKE '%' || $1 || '%')
)
SELECT entity_type, entity_id, label, description, score
FROM hits
ORDER BY score DESC, label ASC
LIMIT $3
"#;

// The unique key includes content_sha256, so an entity/model pair can retain
// several historical rows. Rank those rows before resolving visibility to
// make the logical result one current row per model; this is duplicate
// suppression, not a proof that the selected digest matches every source
// mutation. DEN-1165 tracks the model registry and freshness lifecycle.
const SEMANTIC_SEARCH_SQL: &str = r#"
WITH query_values AS (
    SELECT ordinality::integer AS position, value::double precision AS value
    FROM jsonb_array_elements_text($1::jsonb)
         WITH ORDINALITY AS query_component(value, ordinality)
), query_norm AS (
    SELECT sqrt(sum(value * value)) AS norm
    FROM query_values
), visible_orgs AS (
    SELECT value::uuid AS org_id
    FROM jsonb_array_elements_text($4::jsonb)
), scored AS (
    SELECT
        e.id,
        e.entity_type,
        e.entity_id,
        e.org_id,
        e.embedding_model,
        e.updated_at AS embedding_updated_at,
        sum(component.value::double precision * query_values.value) AS dot_product,
        sqrt(sum(power(component.value::double precision, 2))) AS entity_norm,
        max(query_norm.norm) AS query_norm
    FROM zed_entity_embeddings e
    JOIN LATERAL jsonb_array_elements_text(e.embedding)
         WITH ORDINALITY AS component(value, ordinality) ON true
    JOIN query_values ON query_values.position = component.ordinality
    CROSS JOIN query_norm
    WHERE e.embedding_model = $2
      AND e.embedding_dimensions = $3
    GROUP BY e.id, e.entity_type, e.entity_id, e.org_id, e.embedding_model, e.updated_at
), ranked AS (
    SELECT
        scored.*,
        row_number() OVER (
            PARTITION BY scored.entity_type, scored.entity_id, scored.embedding_model
            ORDER BY scored.embedding_updated_at DESC, scored.id DESC
        ) AS embedding_rank
    FROM scored
), resolved AS (
    SELECT
        ranked.entity_type,
        ranked.entity_id,
        CASE ranked.entity_type
            WHEN 'org' THEN embedded_org.slug
            WHEN 'project' THEN project_org.slug || '/' || embedded_project.slug
            WHEN 'package' THEN package_org.slug || '/' || embedded_package.name
            WHEN 'package_version' THEN version_org.slug || '/' || version_package.name || '@' || embedded_version.version
        END AS label,
        CASE ranked.entity_type
            WHEN 'org' THEN embedded_org.description
            WHEN 'project' THEN embedded_project.description
            WHEN 'package' THEN embedded_package.description
            WHEN 'package_version' THEN version_package.description
        END AS description,
        ranked.dot_product / nullif(ranked.entity_norm * ranked.query_norm, 0) AS score
    FROM ranked
    LEFT JOIN zed_orgs embedded_org
      ON ranked.entity_type = 'org' AND embedded_org.id = ranked.entity_id
    LEFT JOIN zed_projects embedded_project
      ON ranked.entity_type = 'project' AND embedded_project.id = ranked.entity_id
    LEFT JOIN zed_orgs project_org ON project_org.id = embedded_project.org_id
    LEFT JOIN zed_packages embedded_package
      ON ranked.entity_type = 'package' AND embedded_package.id = ranked.entity_id
    LEFT JOIN zed_orgs package_org ON package_org.id = embedded_package.org_id
    LEFT JOIN zed_package_versions embedded_version
      ON ranked.entity_type = 'package_version' AND embedded_version.id = ranked.entity_id
    LEFT JOIN zed_packages version_package ON version_package.id = embedded_version.package_id
    LEFT JOIN zed_orgs version_org ON version_org.id = version_package.org_id
    WHERE
        ranked.embedding_rank = 1
        AND (
            (
                ranked.entity_type = 'org'
                AND embedded_org.id IN (SELECT org_id FROM visible_orgs)
                AND embedded_org.is_soft_deleted = false
            )
            OR (
                ranked.entity_type = 'project'
                AND embedded_project.org_id IN (SELECT org_id FROM visible_orgs)
                AND embedded_project.is_soft_deleted = false
                AND project_org.is_soft_deleted = false
            )
            OR (
                ranked.entity_type = 'package'
                AND (
                    embedded_package.visibility = 'public'
                    OR embedded_package.org_id IN (SELECT org_id FROM visible_orgs)
                )
                AND embedded_package.is_soft_deleted = false
                AND package_org.is_soft_deleted = false
            )
            OR (
                ranked.entity_type = 'package_version'
                AND (
                    version_package.visibility = 'public'
                    OR version_package.org_id IN (SELECT org_id FROM visible_orgs)
                )
                AND version_package.is_soft_deleted = false
                AND version_org.is_soft_deleted = false
            )
        )
)
SELECT entity_type, entity_id, label, description, score
FROM resolved
WHERE label IS NOT NULL AND score IS NOT NULL
ORDER BY score DESC, label ASC, entity_type ASC, entity_id ASC
LIMIT $5
"#;

#[derive(Clone, Debug, PartialEq)]
pub struct RegistrySearchHit {
    pub entity_type: String,
    pub entity_id: Uuid,
    pub label: String,
    pub description: Option<String>,
    pub score: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmbeddingInput {
    pub entity_type: String,
    pub entity_id: Uuid,
    /// Owning organization used as the leading authorization filter.
    pub org_id: Uuid,
    pub embedding_model: String,
    pub embedding: Vec<f32>,
    pub content_sha256: String,
    pub content_preview: Option<String>,
}

/// Visibility-aware text search over canonical registry entities.
///
/// Organizations and projects require an explicit visible organization id;
/// packages and versions additionally allow public rows. The caller computes
/// `visible_org_ids` once from its authorization layer.
pub async fn search_registry(
    context: &ReadContext,
    query: &str,
    visible_org_ids: &[Uuid],
    limit: u64,
) -> Result<Vec<RegistrySearchHit>, OrmError> {
    let query = query.trim().chars().take(200).collect::<String>();
    if query.is_empty() {
        return Ok(Vec::new());
    }
    query_hits(
        context,
        Statement::from_sql_and_values(
            context.connection().get_database_backend(),
            TEXT_SEARCH_SQL,
            [
                Value::String(Some(Box::new(query))),
                Value::Json(Some(Box::new(uuid_array(visible_org_ids)))),
                Value::BigInt(Some(limit.clamp(1, 100) as i64)),
            ],
        ),
    )
    .await
}

/// Visibility-aware cosine search without requiring the PostgreSQL `vector`
/// extension. Embeddings remain canonical JSON arrays; SQL expands matching
/// dimensions and computes cosine similarity directly.
pub async fn semantic_search(
    context: &ReadContext,
    embedding_model_name: &str,
    query_embedding: &[f32],
    visible_org_ids: &[Uuid],
    limit: u64,
) -> Result<Vec<RegistrySearchHit>, OrmError> {
    embedding_model(embedding_model_name)?;
    embedding(query_embedding)?;
    let dimensions = i32::try_from(query_embedding.len())
        .map_err(|_| OrmError::policy("embedding has too many dimensions"))?;

    query_hits(
        context,
        Statement::from_sql_and_values(
            context.connection().get_database_backend(),
            SEMANTIC_SEARCH_SQL,
            [
                Value::Json(Some(Box::new(embedding_json(query_embedding)))),
                Value::String(Some(Box::new(embedding_model_name.to_owned()))),
                Value::Int(Some(dimensions)),
                Value::Json(Some(Box::new(uuid_array(visible_org_ids)))),
                Value::BigInt(Some(limit.clamp(1, 100) as i64)),
            ],
        ),
    )
    .await
}

/// Insert or refresh an embedding without changing the dimensions of an existing
/// model/content identity. A different dimension is rejected atomically by the
/// database; callers must not truncate, pad, or silently overwrite stored vectors.
#[cfg(feature = "read-write")]
pub async fn upsert_embedding(
    context: &WriteContext,
    input: &EmbeddingInput,
) -> Result<Uuid, OrmError> {
    entity_type(&input.entity_type)?;
    embedding_model(&input.embedding_model)?;
    embedding(&input.embedding)?;
    sha256("embedding content SHA-256", &input.content_sha256)?;
    optional_text(
        "embedding content preview",
        input.content_preview.as_deref(),
        4_096,
    )?;
    let dimensions = i32::try_from(input.embedding.len())
        .map_err(|_| OrmError::policy("embedding has too many dimensions"))?;

    let statement = Statement::from_sql_and_values(
        context.connection().get_database_backend(),
        r#"
INSERT INTO zed_entity_embeddings (
    id,
    entity_type,
    entity_id,
    org_id,
    embedding_model,
    embedding,
    embedding_dimensions,
    content_sha256,
    content_preview
) VALUES ($1, $2, $3, $4, $5, $6::jsonb, $7, $8, $9)
ON CONFLICT (entity_type, entity_id, embedding_model, content_sha256)
DO UPDATE SET
    org_id = EXCLUDED.org_id,
    embedding = EXCLUDED.embedding,
    embedding_dimensions = EXCLUDED.embedding_dimensions,
    content_preview = EXCLUDED.content_preview,
    updated_at = clock_timestamp()
WHERE zed_entity_embeddings.embedding_dimensions = EXCLUDED.embedding_dimensions
RETURNING id
"#,
        [
            Value::Uuid(Some(Box::new(Uuid::new_v4()))),
            Value::String(Some(Box::new(input.entity_type.clone()))),
            Value::Uuid(Some(Box::new(input.entity_id))),
            Value::Uuid(Some(Box::new(input.org_id))),
            Value::String(Some(Box::new(input.embedding_model.clone()))),
            Value::Json(Some(Box::new(embedding_json(&input.embedding)))),
            Value::Int(Some(dimensions)),
            Value::String(Some(Box::new(input.content_sha256.clone()))),
            Value::String(input.content_preview.clone().map(Box::new)),
        ],
    );
    context
        .connection()
        .query_one(statement)
        .await
        .map_err(OrmError::from_db_err)?
        .ok_or_else(|| {
            OrmError::policy(
                "embedding dimensions conflict with the existing model/content identity; \
                 retain the existing dimensions or use a distinct versioned model identity",
            )
        })?
        .try_get("", "id")
        .map_err(OrmError::database)
}

async fn query_hits(
    context: &ReadContext,
    statement: Statement,
) -> Result<Vec<RegistrySearchHit>, OrmError> {
    context
        .connection()
        .query_all(statement)
        .await
        .map_err(OrmError::from_db_err)?
        .into_iter()
        .map(|row| {
            Ok(RegistrySearchHit {
                entity_type: row.try_get("", "entity_type").map_err(OrmError::database)?,
                entity_id: row.try_get("", "entity_id").map_err(OrmError::database)?,
                label: row.try_get("", "label").map_err(OrmError::database)?,
                description: row.try_get("", "description").map_err(OrmError::database)?,
                score: row.try_get("", "score").map_err(OrmError::database)?,
            })
        })
        .collect()
}

fn embedding_json(values: &[f32]) -> Json {
    Json::Array(
        values
            .iter()
            .map(|value| {
                Number::from_f64(f64::from(*value))
                    .map(Json::Number)
                    .expect("embedding validation rejects non-finite values")
            })
            .collect(),
    )
}

fn uuid_array(values: &[Uuid]) -> Json {
    Json::Array(
        values
            .iter()
            .map(|value| Json::String(value.to_string()))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_semantic_search_has_no_pgvector_dependency() {
        assert!(SEMANTIC_SEARCH_SQL.contains("zed_entity_embeddings"));
        assert!(SEMANTIC_SEARCH_SQL.contains("jsonb_array_elements_text"));
        assert!(!SEMANTIC_SEARCH_SQL.contains("::vector"));
        assert!(!SEMANTIC_SEARCH_SQL.contains("<=>"));
    }

    #[test]
    fn semantic_search_deduplicates_by_latest_model_row_before_visibility() {
        assert!(SEMANTIC_SEARCH_SQL.contains("row_number() OVER"));
        assert!(SEMANTIC_SEARCH_SQL
            .contains("PARTITION BY scored.entity_type, scored.entity_id, scored.embedding_model"));
        assert!(SEMANTIC_SEARCH_SQL
            .contains("ORDER BY scored.embedding_updated_at DESC, scored.id DESC"));
        assert!(SEMANTIC_SEARCH_SQL.contains("FROM ranked"));
        assert!(SEMANTIC_SEARCH_SQL.contains("ranked.embedding_rank = 1"));
    }

    #[test]
    fn semantic_search_has_a_total_order_for_stable_pagination() {
        assert!(SEMANTIC_SEARCH_SQL
            .contains("ORDER BY score DESC, label ASC, entity_type ASC, entity_id ASC"));
    }

    #[test]
    fn search_uses_prefixed_tables_and_explicit_visibility() {
        for table in [
            "zed_orgs",
            "zed_projects",
            "zed_packages",
            "zed_package_versions",
        ] {
            assert!(TEXT_SEARCH_SQL.contains(table));
        }
        assert!(TEXT_SEARCH_SQL.contains("visibility = 'public'"));
        assert!(!TEXT_SEARCH_SQL.contains("search_document"));
    }

    #[test]
    fn vector_json_preserves_dimensions_and_values() {
        let value = embedding_json(&[1.0, -0.5, 0.25]);
        assert_eq!(value.as_array().map(Vec::len), Some(3));
        assert_eq!(value[1].as_f64(), Some(-0.5));
    }
}
