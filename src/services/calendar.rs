use chrono::{DateTime, Datelike, Duration, NaiveDate, TimeZone, Timelike, Utc};

use crate::{
    dto::plans::PlanRecurrence,
    error::{ApiError, ApiResult},
};

pub fn cycle_end(
    anchor: DateTime<Utc>,
    recurrence: PlanRecurrence,
    cycle_ordinal: i64,
) -> ApiResult<Option<DateTime<Utc>>> {
    if recurrence == PlanRecurrence::None {
        return Ok(None);
    }
    if cycle_ordinal < 1 {
        return Err(ApiError::unprocessable(
            "invalid_cycle_ordinal",
            format!("cycle ordinal {cycle_ordinal} must be positive"),
        ));
    }
    if recurrence == PlanRecurrence::Weekly {
        return anchor
            .checked_add_signed(
                Duration::try_weeks(cycle_ordinal)
                    .ok_or_else(|| calendar_overflow(anchor, recurrence, cycle_ordinal))?,
            )
            .map(Some)
            .ok_or_else(|| calendar_overflow(anchor, recurrence, cycle_ordinal));
    }
    let months_per_cycle: i64 = match recurrence {
        PlanRecurrence::Monthly => 1,
        PlanRecurrence::Quarterly => 3,
        PlanRecurrence::Annually => 12,
        PlanRecurrence::None | PlanRecurrence::Weekly => unreachable!("handled above"),
    };
    let months = months_per_cycle
        .checked_mul(cycle_ordinal)
        .ok_or_else(|| calendar_overflow(anchor, recurrence, cycle_ordinal))?;
    add_anchor_months(anchor, months)
        .map(Some)
        .ok_or_else(|| calendar_overflow(anchor, recurrence, cycle_ordinal))
}

pub fn pricing_cycle_bounds(
    anchor: DateTime<Utc>,
    recurrence_rule: &str,
    accepted_at: DateTime<Utc>,
) -> ApiResult<(DateTime<Utc>, DateTime<Utc>)> {
    if accepted_at < anchor {
        return Err(ApiError::unprocessable(
            "invalid_pricing_cycle_time",
            format!("accepted_at {accepted_at} must be at or after cycle anchor {anchor}"),
        ));
    }
    let (frequency, interval) = parse_pricing_recurrence(recurrence_rule)?;
    let mut ordinal = estimated_cycle_ordinal(anchor, accepted_at, frequency, interval);
    while pricing_boundary(anchor, frequency, interval, ordinal + 1)? <= accepted_at {
        ordinal += 1;
    }
    while ordinal > 0 && pricing_boundary(anchor, frequency, interval, ordinal)? > accepted_at {
        ordinal -= 1;
    }
    Ok((
        pricing_boundary(anchor, frequency, interval, ordinal)?,
        pricing_boundary(anchor, frequency, interval, ordinal + 1)?,
    ))
}

#[derive(Clone, Copy)]
enum PricingFrequency {
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

fn parse_pricing_recurrence(rule: &str) -> ApiResult<(PricingFrequency, i64)> {
    let mut parts = rule.split(';');
    let frequency = match parts.next() {
        Some("FREQ=DAILY") => PricingFrequency::Daily,
        Some("FREQ=WEEKLY") => PricingFrequency::Weekly,
        Some("FREQ=MONTHLY") => PricingFrequency::Monthly,
        Some("FREQ=YEARLY") => PricingFrequency::Yearly,
        _ => return Err(invalid_pricing_recurrence(rule)),
    };
    let interval = parts
        .next()
        .and_then(|part| part.strip_prefix("INTERVAL="))
        .and_then(|value| value.parse::<i64>().ok())
        .filter(|value| *value > 0)
        .ok_or_else(|| invalid_pricing_recurrence(rule))?;
    if parts.next().is_some() {
        return Err(invalid_pricing_recurrence(rule));
    }
    Ok((frequency, interval))
}

fn estimated_cycle_ordinal(
    anchor: DateTime<Utc>,
    accepted_at: DateTime<Utc>,
    frequency: PricingFrequency,
    interval: i64,
) -> i64 {
    match frequency {
        PricingFrequency::Daily => (accepted_at - anchor).num_days() / interval,
        PricingFrequency::Weekly => (accepted_at - anchor).num_weeks() / interval,
        PricingFrequency::Monthly => {
            let months = i64::from(accepted_at.year() - anchor.year()) * 12
                + i64::from(accepted_at.month())
                - i64::from(anchor.month());
            months.max(0) / interval
        }
        PricingFrequency::Yearly => i64::from(accepted_at.year() - anchor.year()).max(0) / interval,
    }
}

fn pricing_boundary(
    anchor: DateTime<Utc>,
    frequency: PricingFrequency,
    interval: i64,
    ordinal: i64,
) -> ApiResult<DateTime<Utc>> {
    let factor = interval
        .checked_mul(ordinal)
        .ok_or_else(|| pricing_calendar_overflow(anchor, ordinal))?;
    let boundary = match frequency {
        PricingFrequency::Daily => {
            Duration::try_days(factor).and_then(|duration| anchor.checked_add_signed(duration))
        }
        PricingFrequency::Weekly => {
            Duration::try_weeks(factor).and_then(|duration| anchor.checked_add_signed(duration))
        }
        PricingFrequency::Monthly => add_anchor_months(anchor, factor),
        PricingFrequency::Yearly => {
            add_anchor_months(anchor, factor.checked_mul(12).unwrap_or(i64::MAX))
        }
    };
    boundary.ok_or_else(|| pricing_calendar_overflow(anchor, ordinal))
}

fn invalid_pricing_recurrence(rule: &str) -> ApiError {
    ApiError::unprocessable(
        "invalid_accumulation_cycle",
        format!("recurrence_rule {rule:?} must contain supported FREQ and positive INTERVAL"),
    )
}

fn pricing_calendar_overflow(anchor: DateTime<Utc>, ordinal: i64) -> ApiError {
    ApiError::unprocessable(
        "pricing_calendar_overflow",
        format!("cycle anchor {anchor} and ordinal {ordinal} must form a UTC boundary"),
    )
}

fn add_anchor_months(anchor: DateTime<Utc>, months: i64) -> Option<DateTime<Utc>> {
    let base_month = i64::from(anchor.year()) * 12 + i64::from(anchor.month0());
    let target_month = base_month.checked_add(months)?;
    let year = i32::try_from(target_month.div_euclid(12)).ok()?;
    let month = u32::try_from(target_month.rem_euclid(12) + 1).ok()?;
    let day = anchor.day().min(days_in_month(year, month)?);
    let date = NaiveDate::from_ymd_opt(year, month, day)?;
    Utc.with_ymd_and_hms(
        date.year(),
        date.month(),
        date.day(),
        anchor.hour(),
        anchor.minute(),
        anchor.second(),
    )
    .single()
    .and_then(|value| value.with_nanosecond(anchor.nanosecond()))
}

fn days_in_month(year: i32, month: u32) -> Option<u32> {
    let (next_year, next_month) = if month == 12 {
        (year.checked_add(1)?, 1)
    } else {
        (year, month + 1)
    };
    let first_next = NaiveDate::from_ymd_opt(next_year, next_month, 1)?;
    Some((first_next - Duration::days(1)).day())
}

fn calendar_overflow(anchor: DateTime<Utc>, recurrence: PlanRecurrence, ordinal: i64) -> ApiError {
    ApiError::unprocessable(
        "subscription_calendar_overflow",
        format!("anchor {anchor}, recurrence {recurrence:?}, and ordinal {ordinal} must form a UTC boundary"),
    )
}

#[cfg(test)]
mod tests {
    use chrono::{Datelike, TimeZone, Utc};

    use super::{cycle_end, pricing_cycle_bounds};
    use crate::dto::plans::PlanRecurrence;

    #[test]
    fn subscription_calendar_monthly_preserves_original_anchor_day() {
        let anchor = Utc.with_ymd_and_hms(2026, 1, 31, 10, 0, 0).unwrap();
        let expected = [(2, 28), (3, 31), (4, 30), (5, 31)];
        for (ordinal, (month, day)) in (1_i64..).zip(expected) {
            let end = cycle_end(anchor, PlanRecurrence::Monthly, ordinal)
                .expect("monthly boundary")
                .expect("recurring end");
            assert_eq!((end.month(), end.day()), (month, day));
        }
    }

    #[test]
    fn subscription_calendar_quarterly_and_annual_handle_short_months() {
        let quarterly = Utc.with_ymd_and_hms(2026, 1, 31, 10, 0, 0).unwrap();
        assert_eq!(
            cycle_end(quarterly, PlanRecurrence::Quarterly, 1)
                .expect("quarterly")
                .expect("end")
                .date_naive(),
            chrono::NaiveDate::from_ymd_opt(2026, 4, 30).unwrap()
        );
        let leap = Utc.with_ymd_and_hms(2024, 2, 29, 10, 0, 0).unwrap();
        let expected_days = [28, 28, 28, 29];
        for (ordinal, day) in (1_i64..).zip(expected_days) {
            let end = cycle_end(leap, PlanRecurrence::Annually, ordinal)
                .expect("annual")
                .expect("end");
            assert_eq!(end.day(), day);
        }
    }

    #[test]
    fn subscription_calendar_weekly_and_none_are_exact() {
        let anchor = Utc.with_ymd_and_hms(2026, 9, 4, 12, 30, 0).unwrap();
        assert_eq!(
            cycle_end(anchor, PlanRecurrence::Weekly, 1).expect("weekly"),
            Some(anchor + chrono::Duration::days(7))
        );
        assert_eq!(
            cycle_end(anchor, PlanRecurrence::None, 1).expect("none"),
            None
        );
    }

    #[test]
    fn subscription_calendar_rejects_out_of_range_ordinals_without_panicking() {
        let anchor = Utc.with_ymd_and_hms(2026, 1, 31, 10, 0, 0).unwrap();
        for recurrence in [
            PlanRecurrence::Weekly,
            PlanRecurrence::Monthly,
            PlanRecurrence::Quarterly,
            PlanRecurrence::Annually,
        ] {
            assert_eq!(
                cycle_end(anchor, recurrence, i64::MAX).unwrap_err().code(),
                "subscription_calendar_overflow"
            );
            assert_eq!(
                cycle_end(anchor, recurrence, 0).unwrap_err().code(),
                "invalid_cycle_ordinal"
            );
        }
    }

    #[test]
    fn pricing_cycle_uses_half_open_monthly_boundaries_from_original_anchor() {
        let anchor = Utc.with_ymd_and_hms(2026, 1, 31, 10, 0, 0).unwrap();
        let boundary = Utc.with_ymd_and_hms(2026, 4, 30, 10, 0, 0).unwrap();
        let (start, end) = pricing_cycle_bounds(anchor, "FREQ=MONTHLY;INTERVAL=1", boundary)
            .expect("pricing cycle");
        assert_eq!(start, boundary);
        assert_eq!(end, Utc.with_ymd_and_hms(2026, 5, 31, 10, 0, 0).unwrap());
    }

    #[test]
    fn pricing_cycle_rejects_time_before_anchor_and_invalid_rule() {
        let anchor = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        assert!(pricing_cycle_bounds(
            anchor,
            "FREQ=DAILY;INTERVAL=1",
            anchor - chrono::Duration::seconds(1)
        )
        .is_err());
        assert!(pricing_cycle_bounds(anchor, "FREQ=HOURLY;INTERVAL=1", anchor).is_err());
    }
}
