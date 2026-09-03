use std::fmt;

use chrono::{DateTime, Utc};
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
use utoipa::ToSchema;

use crate::error::{ApiError, ApiResult};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, ToSchema)]
#[schema(value_type = String, example = "1250")]
pub struct CreditUnits(i64);

impl CreditUnits {
    pub fn new(value: i64) -> Self {
        Self(value)
    }

    pub fn value(self) -> i64 {
        self.0
    }

    pub fn checked_add(self, other: Self) -> ApiResult<Self> {
        self.0.checked_add(other.0).map(Self).ok_or_else(|| {
            ApiError::unprocessable(
                "credit_units_overflow",
                format!("{} + {} must fit in signed 64 bits", self.0, other.0),
            )
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, ToSchema)]
#[schema(value_type = String, example = "1000")]
pub struct ItemUnits(i64);

impl ItemUnits {
    pub fn positive(value: i64) -> ApiResult<Self> {
        if value > 0 {
            return Ok(Self(value));
        }
        Err(ApiError::unprocessable(
            "invalid_item_units",
            format!("item_units {value} must be positive"),
        ))
    }

    pub fn value(self) -> i64 {
        self.0
    }

    pub fn checked_add(self, other: Self) -> ApiResult<Self> {
        let value = self.0.checked_add(other.0).ok_or_else(|| {
            ApiError::unprocessable(
                "item_units_overflow",
                format!("{} + {} must fit in signed 64 bits", self.0, other.0),
            )
        })?;
        Self::positive(value)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UtcPeriod {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

impl UtcPeriod {
    pub fn new(start: DateTime<Utc>, end: DateTime<Utc>) -> ApiResult<Self> {
        if start < end {
            return Ok(Self { start, end });
        }
        Err(ApiError::unprocessable(
            "invalid_period",
            format!("period start {start} must be before end {end}"),
        ))
    }
}

impl Serialize for CreditUnits {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0.to_string())
    }
}

impl<'de> Deserialize<'de> for CreditUnits {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserialize_i64_string(deserializer, "credit_units").map(Self)
    }
}

impl Serialize for ItemUnits {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0.to_string())
    }
}

impl<'de> Deserialize<'de> for ItemUnits {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = deserialize_i64_string(deserializer, "item_units")?;
        Self::positive(value).map_err(de::Error::custom)
    }
}

fn deserialize_i64_string<'de, D>(deserializer: D, name: &str) -> Result<i64, D::Error>
where
    D: Deserializer<'de>,
{
    struct DecimalVisitor<'a>(&'a str);

    impl de::Visitor<'_> for DecimalVisitor<'_> {
        type Value = i64;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(formatter, "{} as a signed 64-bit decimal string", self.0)
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            value.parse::<i64>().map_err(|_| {
                E::custom(format!(
                    "{} value {value:?} must be a signed 64-bit decimal string",
                    self.0
                ))
            })
        }
    }

    deserializer.deserialize_str(DecimalVisitor(name))
}

#[cfg(test)]
mod tests {
    use chrono::{TimeDelta, Utc};

    use super::{CreditUnits, ItemUnits, UtcPeriod};

    #[test]
    fn credit_units_use_decimal_json_strings() {
        let units: CreditUnits = serde_json::from_str(r#""1250""#).expect("parse credits");
        assert_eq!(units.value(), 1250);
        assert_eq!(
            serde_json::to_string(&units).expect("serialize credits"),
            r#""1250""#
        );
        assert!(serde_json::from_str::<CreditUnits>("1250").is_err());
    }

    #[test]
    fn item_units_are_positive_decimal_strings() {
        let units: ItemUnits = serde_json::from_str(r#""1000""#).expect("parse item units");
        assert_eq!(units.value(), 1000);
        assert!(serde_json::from_str::<ItemUnits>(r#""0""#).is_err());
    }

    #[test]
    fn checked_arithmetic_rejects_overflow() {
        let result = CreditUnits::new(i64::MAX).checked_add(CreditUnits::new(1));
        assert_eq!(
            result.expect_err("overflow must fail").code(),
            "credit_units_overflow"
        );
    }

    #[test]
    fn utc_period_requires_increasing_bounds() {
        let start = Utc::now();
        assert!(UtcPeriod::new(start, start + TimeDelta::seconds(1)).is_ok());
        assert_eq!(
            UtcPeriod::new(start, start)
                .expect_err("empty period must fail")
                .code(),
            "invalid_period"
        );
    }
}
