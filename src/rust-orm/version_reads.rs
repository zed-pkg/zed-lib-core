//! Bounded package-version reads used by aggregate dependency graph pages.
//!
//! `zed_packages.latest_version` is maintained by the canonical PostgreSQL
//! rollup trigger from the newest non-yanked release. Aggregate pages receive
//! those exact `(package_id, version)` coordinates and resolve them in one
//! pairwise query rather than issuing one round trip per package.

use std::collections::BTreeSet;

use sea_orm::{ColumnTrait, Condition, EntityTrait, QueryFilter, QueryOrder, QuerySelect, Select};
use uuid::Uuid;

use crate::entities::package_version;
use crate::read::PAGE_LIMIT;
use crate::{OrmError, ReadContext};

/// Maximum exact package-version coordinates accepted by one unpaginated read.
///
/// This deliberately shares the canonical registry page budget. Callers that
/// need more must paginate or add a purpose-built aggregate query rather than
/// expanding an unbounded `OR` predicate.
pub const EXACT_VERSION_COORDINATE_LIMIT: usize = PAGE_LIMIT as usize;

/// Resolve exact current, non-yanked package versions in one bounded query.
///
/// Coordinates are pairwise: `(package_a, "1.0.0")` and
/// `(package_b, "2.0.0")` cannot accidentally match `package_a@2.0.0`.
/// Missing or yanked rows are omitted. Duplicate coordinates are collapsed
/// before the query is built.
pub async fn exact_unyanked_package_versions(
    context: &ReadContext,
    coordinates: &[(Uuid, String)],
) -> Result<Vec<package_version::Model>, OrmError> {
    let query = exact_unyanked_package_versions_query(coordinates)?;
    let Some(query) = query else {
        return Ok(Vec::new());
    };

    query
        .all(context.connection())
        .await
        .map_err(OrmError::from_db_err)
}

fn exact_unyanked_package_versions_query(
    coordinates: &[(Uuid, String)],
) -> Result<Option<Select<package_version::Entity>>, OrmError> {
    if coordinates.len() > EXACT_VERSION_COORDINATE_LIMIT {
        return Err(OrmError::policy(format!(
            "exact package-version read received {} coordinates; limit is {}",
            coordinates.len(),
            EXACT_VERSION_COORDINATE_LIMIT
        )));
    }

    let unique = coordinates.iter().cloned().collect::<BTreeSet<_>>();
    if unique.is_empty() {
        return Ok(None);
    }

    let exact_coordinates = unique.into_iter().fold(Condition::any(), |condition, (package_id, version)| {
        condition.add(
            Condition::all()
                .add(package_version::Column::PackageId.eq(package_id))
                .add(package_version::Column::Version.eq(version)),
        )
    });

    Ok(Some(
        package_version::Entity::find()
            .filter(exact_coordinates)
            .filter(package_version::Column::Yanked.eq(false))
            .order_by_asc(package_version::Column::PackageId)
            .limit(PAGE_LIMIT),
    ))
}

#[cfg(test)]
mod tests {
    use sea_orm::{DbBackend, QueryTrait};

    use super::*;

    #[test]
    fn empty_coordinates_skip_the_database_query() {
        assert!(
            exact_unyanked_package_versions_query(&[])
                .expect("empty input is valid")
                .is_none()
        );
    }

    #[test]
    fn oversized_coordinate_sets_are_rejected_before_querying() {
        let coordinates = (0..=EXACT_VERSION_COORDINATE_LIMIT)
            .map(|index| (Uuid::from_u128(index as u128 + 1), "1.0.0".to_owned()))
            .collect::<Vec<_>>();

        let error = exact_unyanked_package_versions_query(&coordinates)
            .expect_err("unpaginated exact reads must stay bounded");
        assert!(matches!(error, OrmError::PolicyViolation(_)));
        assert!(error.to_string().contains("limit is 100"));
    }

    #[test]
    fn query_keeps_coordinates_pairwise_and_excludes_yanked_rows() {
        let first = Uuid::from_u128(1);
        let second = Uuid::from_u128(2);
        let query = exact_unyanked_package_versions_query(&[
            (first, "1.0.0".to_owned()),
            (second, "2.0.0".to_owned()),
            (first, "1.0.0".to_owned()),
        ])
        .expect("coordinates are valid")
        .expect("non-empty coordinates build a query");

        let statement = query.build(DbBackend::Postgres).to_string();
        assert!(statement.contains("OR"));
        assert!(statement.contains("AND"));
        assert!(statement.contains("package_id"));
        assert!(statement.contains("version"));
        assert!(statement.contains("yanked"));
        assert!(statement.contains("FALSE"));
        assert!(statement.contains("LIMIT 100"));
        assert_eq!(statement.matches(&first.to_string()).count(), 1);
        assert_eq!(statement.matches(&second.to_string()).count(), 1);
    }
}
