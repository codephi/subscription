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
            .checked_add_signed(Duration::weeks(cycle_ordinal))
            .map(Some)
            .ok_or_else(|| calendar_overflow(anchor, recurrence, cycle_ordinal));
    }
    let months_per_cycle = match recurrence {
        PlanRecurrence::Monthly => 1,
        PlanRecurrence::Quarterly => 3,
        PlanRecurrence::Annually => 12,
        PlanRecurrence::None | PlanRecurrence::Weekly => unreachable!("handled above"),
    };
    add_anchor_months(anchor, months_per_cycle * cycle_ordinal)
        .map(Some)
        .ok_or_else(|| calendar_overflow(anchor, recurrence, cycle_ordinal))
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

    use super::cycle_end;
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
}
