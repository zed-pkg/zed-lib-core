//! Pure runtime-configuration materialization shared across Zed executables.
//!
//! This module never reads the process environment. Executables must run
//! `flags-2-env` first, capture an immutable key/value map, and then call the
//! server-only materializer. Client builds receive only [`PublicRuntimeConfig`].

use std::collections::BTreeMap;
use std::fmt;

const SENSITIVE_KEY_FRAGMENTS: &[&str] = &[
    "credential",
    "database_url",
    "password",
    "private_key",
    "secret",
    "token",
];

#[derive(Clone, Default, Eq, PartialEq)]
pub struct PublicRuntimeConfig {
    values: BTreeMap<String, String>,
}

impl PublicRuntimeConfig {
    pub fn from_entries<I, K, V>(entries: I) -> Result<Self, ConfigError>
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        let mut values = BTreeMap::new();
        for (key, value) in entries {
            let key = key.into();
            validate_public_key(&key)?;
            let value = value.into();
            if value.is_empty() {
                return Err(ConfigError::EmptyValue { key });
            }
            if values.insert(key.clone(), value).is_some() {
                return Err(ConfigError::DuplicateOutputKey { key });
            }
        }
        Ok(Self { values })
    }

    #[must_use]
    pub fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = (&str, &str)> {
        self.values
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.values.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

impl fmt::Debug for PublicRuntimeConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PublicRuntimeConfig")
            .field("values", &self.values)
            .finish()
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ConfigError {
    InvalidSourceKey { key: String },
    InvalidOutputKey { key: String },
    SensitivePublicKey { key: String },
    DuplicateSourceKey { key: String },
    DuplicateOutputKey { key: String },
    MissingRequiredValue { key: String },
    EmptyValue { key: String },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSourceKey { key } => write!(
                formatter,
                "configuration source key `{key}` must use uppercase ASCII letters, digits, and underscores"
            ),
            Self::InvalidOutputKey { key } => write!(
                formatter,
                "configuration output key `{key}` must use lowercase ASCII letters, digits, dots, dashes, and underscores"
            ),
            Self::SensitivePublicKey { key } => {
                write!(
                    formatter,
                    "public configuration key `{key}` is secret-shaped"
                )
            }
            Self::DuplicateSourceKey { key } => {
                write!(
                    formatter,
                    "configuration source key `{key}` is declared twice"
                )
            }
            Self::DuplicateOutputKey { key } => {
                write!(
                    formatter,
                    "configuration output key `{key}` is declared twice"
                )
            }
            Self::MissingRequiredValue { key } => {
                write!(formatter, "required configuration value `{key}` is missing")
            }
            Self::EmptyValue { key } => {
                write!(formatter, "configuration value `{key}` cannot be empty")
            }
        }
    }
}

impl std::error::Error for ConfigError {}

fn validate_public_key(key: &str) -> Result<(), ConfigError> {
    if key.is_empty()
        || !key.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'-' | b'_')
        })
    {
        return Err(ConfigError::InvalidOutputKey {
            key: key.to_owned(),
        });
    }
    if SENSITIVE_KEY_FRAGMENTS
        .iter()
        .any(|fragment| key.contains(fragment))
    {
        return Err(ConfigError::SensitivePublicKey {
            key: key.to_owned(),
        });
    }
    Ok(())
}

#[cfg(feature = "server-config")]
pub mod server {
    use std::collections::{BTreeMap, BTreeSet};
    use std::fmt;

    use super::{ConfigError, PublicRuntimeConfig, validate_public_key};

    #[derive(Debug, Clone, Copy, Eq, PartialEq)]
    pub enum Exposure {
        Public,
        Server,
        Secret,
    }

    #[derive(Debug, Clone, Copy, Eq, PartialEq)]
    pub enum Requirement {
        Required,
        Optional,
        Default(&'static str),
    }

    #[derive(Debug, Clone, Copy, Eq, PartialEq)]
    pub struct FieldSpec {
        source_key: &'static str,
        output_key: &'static str,
        exposure: Exposure,
        requirement: Requirement,
    }

    impl FieldSpec {
        #[must_use]
        pub const fn public(source_key: &'static str, output_key: &'static str) -> Self {
            Self {
                source_key,
                output_key,
                exposure: Exposure::Public,
                requirement: Requirement::Required,
            }
        }

        #[must_use]
        pub const fn server(source_key: &'static str, output_key: &'static str) -> Self {
            Self {
                source_key,
                output_key,
                exposure: Exposure::Server,
                requirement: Requirement::Required,
            }
        }

        #[must_use]
        pub const fn secret(source_key: &'static str) -> Self {
            Self {
                source_key,
                output_key: source_key,
                exposure: Exposure::Secret,
                requirement: Requirement::Required,
            }
        }

        #[must_use]
        pub const fn optional(mut self) -> Self {
            self.requirement = Requirement::Optional;
            self
        }

        #[must_use]
        pub const fn with_default(mut self, value: &'static str) -> Self {
            self.requirement = Requirement::Default(value);
            self
        }
    }

    #[derive(Clone, Default, Eq, PartialEq)]
    pub struct ServerRuntimeConfig {
        public: PublicRuntimeConfig,
        server: BTreeMap<String, String>,
        secrets: BTreeMap<String, String>,
    }

    impl ServerRuntimeConfig {
        pub fn materialize(
            specs: &[FieldSpec],
            source: &BTreeMap<String, String>,
        ) -> Result<Self, ConfigError> {
            let mut source_keys = BTreeSet::new();
            let mut output_keys = BTreeSet::new();
            let mut public_entries = Vec::new();
            let mut server = BTreeMap::new();
            let mut secrets = BTreeMap::new();

            for spec in specs {
                validate_source_key(spec.source_key)?;
                if !source_keys.insert(spec.source_key) {
                    return Err(ConfigError::DuplicateSourceKey {
                        key: spec.source_key.to_owned(),
                    });
                }
                if !output_keys.insert(spec.output_key) {
                    return Err(ConfigError::DuplicateOutputKey {
                        key: spec.output_key.to_owned(),
                    });
                }
                validate_output_key(spec.output_key, spec.exposure)?;

                let value = resolve_value(spec, source)?;
                let Some(value) = value else {
                    continue;
                };
                match spec.exposure {
                    Exposure::Public => {
                        public_entries.push((spec.output_key.to_owned(), value));
                    }
                    Exposure::Server => {
                        server.insert(spec.output_key.to_owned(), value);
                    }
                    Exposure::Secret => {
                        secrets.insert(spec.source_key.to_owned(), value);
                    }
                }
            }

            Ok(Self {
                public: PublicRuntimeConfig::from_entries(public_entries)?,
                server,
                secrets,
            })
        }

        #[must_use]
        pub fn public_projection(&self) -> PublicRuntimeConfig {
            self.public.clone()
        }

        #[must_use]
        pub fn value(&self, key: &str) -> Option<&str> {
            self.public
                .get(key)
                .or_else(|| self.server.get(key).map(String::as_str))
        }

        #[must_use]
        pub fn secret(&self, source_key: &str) -> Option<SecretValue<'_>> {
            self.secrets
                .get(source_key)
                .map(|value| SecretValue(value.as_str()))
        }
    }

    impl fmt::Debug for ServerRuntimeConfig {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter
                .debug_struct("ServerRuntimeConfig")
                .field("public", &self.public)
                .field("server_keys", &self.server.keys().collect::<Vec<_>>())
                .field("secret_keys", &self.secrets.keys().collect::<Vec<_>>())
                .finish()
        }
    }

    #[derive(Clone, Copy, Eq, PartialEq)]
    pub struct SecretValue<'a>(&'a str);

    impl<'a> SecretValue<'a> {
        #[must_use]
        pub const fn expose(self) -> &'a str {
            self.0
        }
    }

    impl fmt::Debug for SecretValue<'_> {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("SecretValue(<redacted>)")
        }
    }

    fn validate_output_key(key: &str, exposure: Exposure) -> Result<(), ConfigError> {
        if exposure == Exposure::Secret {
            return Ok(());
        }
        if key.is_empty()
            || !key.bytes().all(|byte| {
                byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || matches!(byte, b'.' | b'-' | b'_')
            })
        {
            return Err(ConfigError::InvalidOutputKey {
                key: key.to_owned(),
            });
        }
        if exposure == Exposure::Public {
            validate_public_key(key)?;
        }
        Ok(())
    }

    fn validate_source_key(key: &str) -> Result<(), ConfigError> {
        if key.is_empty()
            || !key
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
        {
            return Err(ConfigError::InvalidSourceKey {
                key: key.to_owned(),
            });
        }
        Ok(())
    }

    fn resolve_value(
        spec: &FieldSpec,
        source: &BTreeMap<String, String>,
    ) -> Result<Option<String>, ConfigError> {
        let found = source.get(spec.source_key).cloned();
        let value = match (found, spec.requirement) {
            (Some(value), _) => Some(value),
            (None, Requirement::Required) => {
                return Err(ConfigError::MissingRequiredValue {
                    key: spec.source_key.to_owned(),
                });
            }
            (None, Requirement::Optional) => None,
            (None, Requirement::Default(value)) => Some(value.to_owned()),
        };
        if value.as_deref().is_some_and(str::is_empty) {
            return Err(ConfigError::EmptyValue {
                key: spec.source_key.to_owned(),
            });
        }
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_config_rejects_secret_shaped_keys() {
        for key in ["api_token", "database_url", "client_secret", "private_key"] {
            assert!(matches!(
                PublicRuntimeConfig::from_entries([(key, "value")]),
                Err(ConfigError::SensitivePublicKey { .. })
            ));
        }
    }

    #[test]
    fn public_config_is_deterministic() {
        let config = PublicRuntimeConfig::from_entries([
            ("registry_origin", "https://registry.example"),
            ("service_name", "zed"),
        ])
        .expect("valid public config");
        assert_eq!(
            config.get("registry_origin"),
            Some("https://registry.example")
        );
        assert_eq!(
            config.iter().next(),
            Some(("registry_origin", "https://registry.example"))
        );
    }

    #[cfg(feature = "server-config")]
    mod server_tests {
        use std::collections::BTreeMap;

        use super::super::server::{FieldSpec, ServerRuntimeConfig};

        #[test]
        fn materialization_separates_public_server_and_secret_values() {
            let specs = [
                FieldSpec::public("PUBLIC_BASE_URL", "public_base_url"),
                FieldSpec::server("DB_MAX_CONNECTIONS", "db_max_connections").with_default("10"),
                FieldSpec::secret("DATABASE_URL"),
            ];
            let mut source = BTreeMap::from([
                (
                    "PUBLIC_BASE_URL".to_owned(),
                    "https://registry.example".to_owned(),
                ),
                ("DATABASE_URL".to_owned(), "postgres://private".to_owned()),
            ]);
            let config = ServerRuntimeConfig::materialize(&specs, &source).expect("materialize");
            source.insert(
                "PUBLIC_BASE_URL".to_owned(),
                "https://changed.example".to_owned(),
            );

            assert_eq!(
                config.value("public_base_url"),
                Some("https://registry.example")
            );
            assert_eq!(config.value("db_max_connections"), Some("10"));
            assert_eq!(
                config.secret("DATABASE_URL").expect("secret").expose(),
                "postgres://private"
            );
            assert_eq!(config.public_projection().len(), 1);
            assert!(!format!("{config:?}").contains("postgres://private"));
        }

        #[test]
        fn duplicate_and_missing_declarations_fail_closed() {
            let duplicate = [
                FieldSpec::server("RUST_LOG", "rust_log"),
                FieldSpec::server("RUST_LOG", "log_filter"),
            ];
            assert!(ServerRuntimeConfig::materialize(&duplicate, &BTreeMap::new()).is_err());
            assert!(
                ServerRuntimeConfig::materialize(
                    &[FieldSpec::secret("DATABASE_URL")],
                    &BTreeMap::new(),
                )
                .is_err()
            );
        }
    }
}
