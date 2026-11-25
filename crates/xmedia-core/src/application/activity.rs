use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use jiff::civil::Date;
use jiff::tz::TimeZone;
use jiff::{Timestamp, ToSpan};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::domain::activity::{UserActivityDaily, UserActivityRepository};
use crate::domain::error::{DomainError, DomainResult};
use crate::domain::library::MediaKind;
use crate::domain::user::UserId;

const MAX_HEARTBEAT_DELTA_SECS: u32 = 600;
const MAX_DAILY_DURATION_SECS: u64 = 86_400;
const MAX_TITLE_CHARS: usize = 120;
const DEFAULT_ROLLING_DAYS: i64 = 365;

#[derive(Debug, Clone, Deserialize)]
pub struct RecordActivityInput {
    pub media_kind: MediaKind,
    /// Client local calendar date in `YYYY-MM-DD`.
    pub date: String,
    /// Incremental active seconds to add (1..=600).
    pub delta_secs: u32,
    #[serde(default)]
    pub new_session: bool,
    #[serde(default)]
    pub item_title: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ActivityHeatmapQuery {
    pub media_kind: MediaKind,
    /// Optional calendar year (e.g. `2026`). When omitted, returns rolling last 365 days.
    pub year: Option<i16>,
    /// Client local "today" in `YYYY-MM-DD` so streaks & rolling window match client timezone.
    pub today: Option<String>,
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

type KeyLockMap = Mutex<HashMap<(UserId, MediaKind, String), Arc<Mutex<()>>>>;

pub struct ActivityService {
    repo: Arc<dyn UserActivityRepository>,
    key_locks: KeyLockMap,
}

impl ActivityService {
    pub fn new(repo: Arc<dyn UserActivityRepository>) -> Self {
        Self {
            repo,
            key_locks: Mutex::new(HashMap::new()),
        }
    }

    async fn lock_for(&self, user_id: UserId, media_kind: MediaKind, date: &str) -> Arc<Mutex<()>> {
        let mut map = self.key_locks.lock().await;
        if map.len() > 2048 {
            map.retain(|_, v| Arc::strong_count(v) > 1);
        }
        map.entry((user_id, media_kind, date.to_string()))
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    pub async fn record_heartbeat(
        &self,
        user_id: UserId,
        input: RecordActivityInput,
    ) -> DomainResult<ActivityDayDto> {
        if input.delta_secs == 0 || input.delta_secs > MAX_HEARTBEAT_DELTA_SECS {
            return Err(DomainError::Validation(format!(
                "delta_secs must be between 1 and {MAX_HEARTBEAT_DELTA_SECS}"
            )));
        }

        let parsed_date = parse_calendar_date(&input.date)?;
        let utc_today = Timestamp::now().to_zoned(TimeZone::UTC).date();
        let max_allowed = utc_today
            .checked_add(2.days())
            .map_err(|e| DomainError::Validation(format!("date overflow: {e}")))?;
        if parsed_date > max_allowed {
            return Err(DomainError::Validation(
                "activity date cannot be in the future".into(),
            ));
        }
        let date_str = parsed_date.to_string();

        let normalized_title = input
            .item_title
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.chars().take(MAX_TITLE_CHARS).collect::<String>());

        let lock = self.lock_for(user_id, input.media_kind, &date_str).await;
        let _guard = lock.lock().await;

        let now = Timestamp::now();
        let mut record = match self
            .repo
            .find_by_key(user_id, input.media_kind, &date_str)
            .await?
        {
            Some(mut existing) => {
                existing.duration_secs = existing
                    .duration_secs
                    .saturating_add(u64::from(input.delta_secs))
                    .min(MAX_DAILY_DURATION_SECS);
                if input.new_session || existing.active_sessions == 0 {
                    existing.active_sessions = existing.active_sessions.saturating_add(1);
                }
                if normalized_title.is_some() {
                    existing.last_item_title = normalized_title;
                }
                existing.updated_at = now;
                existing
            }
            None => UserActivityDaily {
                id: Uuid::now_v7(),
                user_id,
                media_kind: input.media_kind,
                activity_date: date_str,
                duration_secs: u64::from(input.delta_secs).min(MAX_DAILY_DURATION_SECS),
                active_sessions: 1,
                last_item_title: normalized_title,
                updated_at: now,
            },
        };

        // Ensure duration stays bounded even if persisted value was modified externally.
        record.duration_secs = record.duration_secs.min(MAX_DAILY_DURATION_SECS);
        self.repo.save(&record).await?;

        Ok(ActivityDayDto {
            date: record.activity_date,
            duration_secs: record.duration_secs,
            active_sessions: record.active_sessions,
            last_item_title: record.last_item_title,
        })
    }

    pub async fn get_heatmap(
        &self,
        user_id: UserId,
        query: ActivityHeatmapQuery,
    ) -> DomainResult<ActivityHeatmapDto> {
        let today = match query
            .today
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            Some(raw) => parse_calendar_date(raw)?,
            None => Timestamp::now().to_zoned(TimeZone::UTC).date(),
        };

        let (start_date, end_date) = match query.year {
            Some(y) => {
                if !(2000..=2100).contains(&y) {
                    return Err(DomainError::Validation("year out of range".into()));
                }
                let start = Date::new(y, 1, 1)
                    .map_err(|e| DomainError::Validation(format!("invalid year {y}: {e}")))?;
                let end = Date::new(y, 12, 31)
                    .map_err(|e| DomainError::Validation(format!("invalid year {y}: {e}")))?;
                (start, end)
            }
            None => {
                let start = today
                    .checked_sub((DEFAULT_ROLLING_DAYS - 1).days())
                    .map_err(|e| DomainError::Validation(format!("date underflow: {e}")))?;
                (start, today)
            }
        };

        let all_records = self
            .repo
            .list_by_user_and_domain(user_id, query.media_kind)
            .await?;

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
        let mut peak_day: Option<ActivityDayDto> = None;
        let mut longest_streak = 0u32;
        let mut run_streak = 0u32;
        let mut prev_date: Option<Date> = None;

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
            if run_streak > longest_streak {
                longest_streak = run_streak;
            }
            prev_date = Some(*d);
        }

        let current_streak = compute_current_streak(&all_active_dates, today);
        let active_days = u32::try_from(range_days.len()).unwrap_or(u32::MAX);
        let avg_active_day_secs = if active_days > 0 {
            total_secs / u64::from(active_days)
        } else {
            0
        };
        let available_years: Vec<i16> = years_set.into_iter().rev().collect();

        Ok(ActivityHeatmapDto {
            media_kind: query.media_kind,
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
                available_years,
            },
            weekday_totals,
        })
    }

    pub async fn purge_user(&self, user_id: UserId) -> DomainResult<()> {
        self.repo.delete_by_user(user_id).await?;
        let mut locks = self.key_locks.lock().await;
        locks.retain(|(uid, _, _), _| *uid != user_id);
        Ok(())
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;

    #[derive(Default)]
    struct MemActivityRepo {
        rows: Mutex<Vec<UserActivityDaily>>,
    }

    #[async_trait]
    impl UserActivityRepository for MemActivityRepo {
        async fn list_by_user_and_domain(
            &self,
            user_id: UserId,
            media_kind: MediaKind,
        ) -> DomainResult<Vec<UserActivityDaily>> {
            let guard = self.rows.lock().await;
            Ok(guard
                .iter()
                .filter(|r| r.user_id == user_id && r.media_kind == media_kind)
                .cloned()
                .collect())
        }

        async fn find_by_key(
            &self,
            user_id: UserId,
            media_kind: MediaKind,
            activity_date: &str,
        ) -> DomainResult<Option<UserActivityDaily>> {
            let guard = self.rows.lock().await;
            Ok(guard
                .iter()
                .find(|r| {
                    r.user_id == user_id
                        && r.media_kind == media_kind
                        && r.activity_date == activity_date
                })
                .cloned())
        }

        async fn save(&self, record: &UserActivityDaily) -> DomainResult<()> {
            let mut guard = self.rows.lock().await;
            if let Some(existing) = guard.iter_mut().find(|r| r.id == record.id) {
                *existing = record.clone();
            } else {
                guard.push(record.clone());
            }
            Ok(())
        }

        async fn delete_by_user(&self, user_id: UserId) -> DomainResult<()> {
            let mut guard = self.rows.lock().await;
            guard.retain(|r| r.user_id != user_id);
            Ok(())
        }
    }

    #[tokio::test]
    async fn heartbeat_accumulates_and_computes_streaks_and_weekdays() {
        let repo = Arc::new(MemActivityRepo::default());
        let svc = ActivityService::new(repo);
        let uid = UserId::new();
        let today = Timestamp::now().to_zoned(TimeZone::UTC).date();
        let d0 = today.to_string();
        let d1 = today.checked_sub(1.days()).unwrap().to_string();
        let d2 = today.checked_sub(2.days()).unwrap().to_string();
        let d10 = today.checked_sub(10.days()).unwrap().to_string();

        // Accumulate two heartbeats on today
        let r1 = svc
            .record_heartbeat(
                uid,
                RecordActivityInput {
                    media_kind: MediaKind::Book,
                    date: d0.clone(),
                    delta_secs: 120,
                    new_session: true,
                    item_title: Some("天启预报".into()),
                },
            )
            .await
            .unwrap();
        assert_eq!(r1.duration_secs, 120);
        assert_eq!(r1.active_sessions, 1);

        let r2 = svc
            .record_heartbeat(
                uid,
                RecordActivityInput {
                    media_kind: MediaKind::Book,
                    date: d0.clone(),
                    delta_secs: 60,
                    new_session: false,
                    item_title: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(r2.duration_secs, 180);
        assert_eq!(r2.active_sessions, 1);
        assert_eq!(r2.last_item_title.as_deref(), Some("天启预报"));

        // Add previous days to build a 3-day streak + 1 older peak day
        for (date, delta) in [(d1, 300), (d2, 240), (d10, 600)] {
            svc.record_heartbeat(
                uid,
                RecordActivityInput {
                    media_kind: MediaKind::Book,
                    date,
                    delta_secs: delta,
                    new_session: true,
                    item_title: None,
                },
            )
            .await
            .unwrap();
        }

        let hm = svc
            .get_heatmap(
                uid,
                ActivityHeatmapQuery {
                    media_kind: MediaKind::Book,
                    year: None,
                    today: Some(d0),
                },
            )
            .await
            .unwrap();

        assert_eq!(hm.days.len(), 4);
        assert_eq!(hm.summary.total_secs, 180 + 300 + 240 + 600);
        assert_eq!(hm.summary.today_secs, 180);
        assert_eq!(hm.summary.week_secs, 180 + 300 + 240);
        assert_eq!(hm.summary.month_secs, 180 + 300 + 240 + 600);
        assert_eq!(hm.summary.active_days, 4);
        assert_eq!(hm.summary.current_streak, 3);
        assert_eq!(hm.summary.longest_streak, 3);
        assert_eq!(hm.summary.peak_day.as_ref().unwrap().duration_secs, 600);
        assert_eq!(hm.weekday_totals.iter().sum::<u64>(), hm.summary.total_secs);

        // Validation checks
        assert!(
            svc.record_heartbeat(
                uid,
                RecordActivityInput {
                    media_kind: MediaKind::Book,
                    date: "2099-01-01".into(),
                    delta_secs: 30,
                    new_session: false,
                    item_title: None,
                },
            )
            .await
            .is_err()
        );
        assert!(
            svc.record_heartbeat(
                uid,
                RecordActivityInput {
                    media_kind: MediaKind::Book,
                    date: today.to_string(),
                    delta_secs: 0,
                    new_session: false,
                    item_title: None,
                },
            )
            .await
            .is_err()
        );
    }
}
