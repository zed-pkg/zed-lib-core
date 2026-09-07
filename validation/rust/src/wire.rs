//! JSON wire representation rules shared by the public validation models.
//! These helpers do not insert defaults, trim strings, or replace garde's
//! semantic validation. Call `Validate::validate` after deserialization.
use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer};
use std::fmt;

/// A missing property is handled by serde(default), while a present property
/// must decode as a string. Option<String>'s normal null-to-None behavior is
/// not valid for the independently authored optional-but-nonnullable schemas.
pub(crate) fn optional_string<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    String::deserialize(deserializer).map(Some)
}

/// JSON Schema integer is a mathematical value, so 50, 50.0 and 5e1 have the
/// same meaning. Accept integral JSON numbers only, never strings, booleans,
/// fractions, nonfinite numbers or values outside u16. Per-field range rules
/// remain with garde. This boundary targets self-describing formats like JSON.
pub(crate) fn integer_u16<'de, D>(deserializer: D) -> Result<u16, D::Error>
where
    D: Deserializer<'de>,
{
    struct Integer;

    impl<'de> Visitor<'de> for Integer {
        type Value = u16;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("an integral JSON number between 0 and 65535")
        }

        fn visit_u64<E>(self, value: u64) -> Result<u16, E>
        where
            E: de::Error,
        {
            u16::try_from(value).map_err(E::custom)
        }

        fn visit_i64<E>(self, value: i64) -> Result<u16, E>
        where
            E: de::Error,
        {
            u16::try_from(value).map_err(E::custom)
        }

        fn visit_f64<E>(self, value: f64) -> Result<u16, E>
        where
            E: de::Error,
        {
            if value.is_finite()
                && value.fract() == 0.0
                && (0.0..=f64::from(u16::MAX)).contains(&value)
            {
                Ok(value as u16)
            } else {
                Err(E::custom(
                    "expected a finite integral JSON number within u16",
                ))
            }
        }
    }

    deserializer.deserialize_any(Integer)
}
