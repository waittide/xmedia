use std::collections::{BTreeSet, HashSet};

use async_trait::async_trait;
use jiff::civil::Date;
use jiff::tz::TimeZone;
use jiff::{Timestamp, ToSpan};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::error::{DomainError, DomainResult};
use crate::domain::library::MediaKind;
use crate::domain::user::UserId;

const MAX_HEARTBEAT_DELTA_SECS: u32 = 600;
const MAX_DAILY_DURATION_SECS: u64 = 86_400;
const MAX_TITLE_CHARS: usize = 120;
const DEFAULT_ROLLING_DAYS: i64 = 365;

/// Aggregated active time spent by a user in a single media domain on a single local calendar day.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserActivityDaily {
    pub id: Uuid,
    pub user_id: UserId,
    pub media_kind: MediaKind,
    /// Calendar date in `YYYY-MM-DD` (user's local date).
    pub activity_date: String,
    pub duration_secs: u64,
    pub active_sessions: u32,
    pub last_item_title: Option<String>,
    pub updated_at: Timestamp,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActivityDayDto {
    pub date: String,
    pub duration_secs: u64,
    pub active_sessions: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_item_title: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActivitySummaryDto {
    pub total_secs: u64,
    pub today_secs: u64,
    pub week_secs: u64,
    pub month_secs: u64,
    pub active_days: u32,
    pub current_streak: u32,
    pub longest_streak: u32,
    pub avg_active_day_secs: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peak_day: Option<ActivityDayDto>,
    pub available_years: Vec<i16>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActivityHeatmapDto {
    pub media_kind: MediaKind,
    pub today: String,
    pub start_date: String,
    pub end_date: String,
    pub days: Vec<ActivityDayDto>,
    pub summary: ActivitySummaryDto,
    /// Accumulated seconds per weekday in `[start_date, end_date]`, Monday = 0 .. Sunday = 6.
    pub weekday_totals: [u64; 7],
}

fn parse_calendar_date(raw: &str) -> DomainResult<Date> {
    let trimmed = raw.trim();
    if trimmed.len() != 10 {
        return Err(DomainError::Validation(format!(
            "invalid calendar date '{trimmed}', expected YYYY-MM-DD"
        )));
    }
    trimmed.parse::<Date>().map_err(|_| {
        DomainError::Validation(format!(
            "invalid calendar date '{trimmed}', expected YYYY-MM-DD"
        ))
    })
}

fn normalize_item_title(raw: Option<&str>) -> Option<String> {
    raw.map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.chars().take(MAX_TITLE_CHARS).collect())
}

fn resolve_window_bounds(today: Date, year: Option<i16>) -> DomainResult<(Date, Date)> {
    match year {
        Some(y) => {
            if !(2000..=2100).contains(&y) {
                return Err(DomainError::Validation("year out of range".into()));
            }
            let start = Date::new(y, 1, 1)
                .map_err(|e| DomainError::Validation(format!("invalid year {y}: {e}")))?;
            let end = Date::new(y, 12, 31)
                .map_err(|e| DomainError::Validation(format!("invalid year {y}: {e}")))?;
            Ok((start, end))
        }
        None => {
            let start = today
                .checked_sub((DEFAULT_ROLLING_DAYS - 1).days())
                .map_err(|e| DomainError::Validation(format!("date underflow: {e}")))?;
            Ok((start, today))
        }
    }
}

fn compute_current_streak(active_dates: &HashSet<Date>, today: Date) -> u32 {
    let mut cursor = if active_dates.contains(&today) {
        today
    } else if let Ok(yesterday) = today.checked_sub(1.days())
        && active_dates.contains(&yesterday)
    {
        yesterday
    } else {
        return 0;
    };

    let mut streak = 0u32;
    while active_dates.contains(&cursor) {
        streak = streak.saturating_add(1);
        match cursor.checked_sub(1.days()) {
            Ok(prev) => cursor = prev,
            Err(_) => break,
        }
    }
    streak
}

fn check_heartbeat_date(raw_date: &str, delta_secs: u32, now: Timestamp) -> DomainResult<String> {
    if delta_secs == 0 || delta_secs > MAX_HEARTBEAT_DELTA_SECS {
        return Err(DomainError::Validation(format!(
            "delta_secs must be between 1 and {MAX_HEARTBEAT_DELTA_SECS}"
        )));
    }
    let parsed_date = parse_calendar_date(raw_date)?;
    let utc_today = now.to_zoned(TimeZone::UTC).date();
    let max_allowed = utc_today
        .checked_add(2.days())
        .map_err(|e| DomainError::Validation(format!("date overflow: {e}")))?;
    if parsed_date > max_allowed {
        return Err(DomainError::Validation(
            "activity date cannot be in the future".into(),
        ));
    }
    Ok(parsed_date.to_string())
}

#[derive(Debug, Clone, Copy)]
pub struct HeartbeatEvent<'a> {
    pub user_id: UserId,
    pub media_kind: MediaKind,
    pub date_str: &'a str,
    pub delta_secs: u32,
    pub new_session: bool,
    pub item_title: Option<&'a str>,
    pub now: Timestamp,
}

impl UserActivityDaily {
    pub fn prepare_date(raw_date: &str, delta_secs: u32, now: Timestamp) -> DomainResult<String> {
        check_heartbeat_date(raw_date, delta_secs, now)
    }

    pub fn record_heartbeat(existing: Option<Self>, event: HeartbeatEvent<'_>) -> Self {
        let normalized_title = normalize_item_title(event.item_title);
        let mut record = match existing {
            Some(mut row) => {
                row.duration_secs = row
                    .duration_secs
                    .saturating_add(u64::from(event.delta_secs))
                    .min(MAX_DAILY_DURATION_SECS);
                if event.new_session || row.active_sessions == 0 {
                    row.active_sessions = row.active_sessions.saturating_add(1);
                }
                if normalized_title.is_some() {
                    row.last_item_title = normalized_title;
                }
                row.updated_at = event.now;
                row
            }
            None => Self {
                id: Uuid::now_v7(),
                user_id: event.user_id,
                media_kind: event.media_kind,
                activity_date: event.date_str.to_owned(),
                duration_secs: u64::from(event.delta_secs).min(MAX_DAILY_DURATION_SECS),
                active_sessions: 1,
                last_item_title: normalized_title,
                updated_at: event.now,
            },
        };
        record.duration_secs = record.duration_secs.min(MAX_DAILY_DURATION_SECS);
        record
    }
}

impl ActivityHeatmapDto {
    pub fn compute(
        media_kind: MediaKind,
        today_raw: Option<&str>,
        year: Option<i16>,
        all_records: Vec<UserActivityDaily>,
        now: Timestamp,
    ) -> DomainResult<Self> {
        let today = match today_raw.map(str::trim).filter(|s| !s.is_empty()) {
            Some(raw) => parse_calendar_date(raw)?,
            None => now.to_zoned(TimeZone::UTC).date(),
        };
        let (start_date, end_date) = resolve_window_bounds(today, year)?;

        let mut years_set = BTreeSet::new();
        years_set.insert(today.year());

        let mut all_active_dates = HashSet::new();
        let mut today_secs = 0u64;
        let mut week_secs = 0u64;
        let mut month_secs = 0u64;
        let week_start = today.checked_sub(6.days()).unwrap_or(start_date);
        let month_start = today.checked_sub(29.days()).unwrap_or(start_date);

        let mut range_days: Vec<(Date, ActivityDayDto)> = Vec::new();
        let mut weekday_totals = [0u64; 7];

        for rec in all_records {
            if rec.duration_secs == 0 {
                continue;
            }
            let Ok(d) = parse_calendar_date(&rec.activity_date) else {
                continue;
            };
            years_set.insert(d.year());
            all_active_dates.insert(d);

            if d == today {
                today_secs = today_secs.saturating_add(rec.duration_secs);
            }
            if d >= week_start && d <= today {
                week_secs = week_secs.saturating_add(rec.duration_secs);
            }
            if d >= month_start && d <= today {
                month_secs = month_secs.saturating_add(rec.duration_secs);
            }
            if d >= start_date && d <= end_date {
                let weekday_idx = usize::try_from(d.weekday().to_monday_zero_offset()).unwrap_or(0);
                if weekday_idx < 7 {
                    weekday_totals[weekday_idx] =
                        weekday_totals[weekday_idx].saturating_add(rec.duration_secs);
                }
                range_days.push((
                    d,
                    ActivityDayDto {
                        date: rec.activity_date,
                        duration_secs: rec.duration_secs,
                        active_sessions: rec.active_sessions,
                        last_item_title: rec.last_item_title,
                    },
                ));
            }
        }

        range_days.sort_by_key(|(d, _)| *d);

        let mut total_secs = 0u64;
        let mut longest_streak = 0u32;
        let mut run_streak = 0u32;
        let mut prev_date: Option<Date> = None;
        let mut peak_day: Option<ActivityDayDto> = None;

        for (d, dto) in &range_days {
            total_secs = total_secs.saturating_add(dto.duration_secs);
            if peak_day
                .as_ref()
                .is_none_or(|p| dto.duration_secs > p.duration_secs)
            {
                peak_day = Some(dto.clone());
            }
            if let Some(prev) = prev_date
                && prev.checked_add(1.days()).ok() == Some(*d)
            {
                run_streak = run_streak.saturating_add(1);
            } else {
                run_streak = 1;
            }
            longest_streak = longest_streak.max(run_streak);
            prev_date = Some(*d);
        }

        let current_streak = compute_current_streak(&all_active_dates, today);
        let active_days = u32::try_from(range_days.len()).unwrap_or(u32::MAX);
        let avg_active_day_secs = if active_days > 0 {
            total_secs / u64::from(active_days)
        } else {
            0
        };

        Ok(Self {
            media_kind,
            today: today.to_string(),
            start_date: start_date.to_string(),
            end_date: end_date.to_string(),
            days: range_days.into_iter().map(|(_, dto)| dto).collect(),
            summary: ActivitySummaryDto {
                total_secs,
                today_secs,
                week_secs,
                month_secs,
                active_days,
                current_streak,
                longest_streak,
                avg_active_day_secs,
                peak_day,
                available_years: years_set.into_iter().rev().collect(),
            },
            weekday_totals,
        })
    }
}

#[async_trait]
pub trait UserActivityRepository: Send + Sync {
    async fn list_by_user_and_domain(
        &self,
        user_id: UserId,
        media_kind: MediaKind,
    ) -> DomainResult<Vec<UserActivityDaily>>;

    async fn find_by_key(
        &self,
        user_id: UserId,
        media_kind: MediaKind,
        activity_date: &str,
    ) -> DomainResult<Option<UserActivityDaily>>;

    async fn save(&self, record: &UserActivityDaily) -> DomainResult<()>;

    async fn delete_by_user(&self, user_id: UserId) -> DomainResult<()>;
}
