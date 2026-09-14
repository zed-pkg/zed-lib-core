//! Canonical distributed lock identities for zed-pkg.
//!
//! Local process exclusion remains in `zed-lock`. Cross-host, TTL-bounded, or
//! failover-sensitive coordination uses `ores-locks-and-leases` so every
//! service shares the same key and fencing semantics.

use ores_locks_and_leases::LockKey;

const PREFIX: &str = "zed-pkg";

fn key(domain: &str, name: &str) -> LockKey {
    LockKey::new(format!("{PREFIX}/{domain}/{name}"))
        .expect("static zed lock prefix and validated resource names must fit LockKey")
}

/// Serialize publication of one package coordinate across registry replicas.
pub fn registry_publish(package: &str) -> LockKey {
    key("registry", &format!("publish:{package}"))
}

/// Protect one package/version index mutation while mirrors and metadata catch up.
pub fn registry_version(package: &str, version: &str) -> LockKey {
    key("registry", &format!("version:{package}@{version}"))
}

/// Only one fleet-wide registry migration runner should mutate schema state.
pub fn registry_migration() -> LockKey {
    key("migrations", "registry")
}

/// Singleton maintenance jobs such as stale-upload cleanup or mirror repair.
pub fn singleton_job(job: &str) -> LockKey {
    key("jobs", &format!("singleton:{job}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_namespaced_and_stable() {
        assert_eq!(
            registry_publish("ores-cli").as_str(),
            "zed-pkg/registry/publish:ores-cli"
        );
        assert_eq!(
            registry_version("ores-cli", "1.2.3").as_str(),
            "zed-pkg/registry/version:ores-cli@1.2.3"
        );
        assert_eq!(registry_migration().as_str(), "zed-pkg/migrations/registry");
    }

    #[test]
    fn singleton_jobs_do_not_collide_with_registry_keys() {
        assert_ne!(
            singleton_job("mirror-repair"),
            registry_publish("mirror-repair")
        );
    }
}
