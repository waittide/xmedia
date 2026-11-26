use std::collections::HashMap;
use std::sync::Arc;

use jiff::Timestamp;
use serde::Deserialize;
use tokio::sync::Mutex;

pub use crate::domain::activity::{ActivityDayDto, ActivityHeatmapDto, ActivitySummaryDto};
use crate::domain::activity::{HeartbeatEvent, UserActivityDaily, UserActivityRepository};
use crate::domain::error::DomainResult;
use crate::domain::library::MediaKind;
use crate::domain::user::UserId;

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
        let now = Timestamp::now();
        let date_str = UserActivityDaily::prepare_date(&input.date, input.delta_secs, now)?;
        let lock = self.lock_for(user_id, input.media_kind, &date_str).await;
        let _guard = lock.lock().await;

        let existing = self
            .repo
            .find_by_key(user_id, input.media_kind, &date_str)
            .await?;
        let record = UserActivityDaily::record_heartbeat(
            existing,
            HeartbeatEvent {
                user_id,
                media_kind: input.media_kind,
                date_str: &date_str,
                delta_secs: input.delta_secs,
                new_session: input.new_session,
                item_title: input.item_title.as_deref(),
                now: Timestamp::now(),
            },
        );
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
        let all_records = self
            .repo
            .list_by_user_and_domain(user_id, query.media_kind)
            .await?;
        ActivityHeatmapDto::compute(
            query.media_kind,
            query.today.as_deref(),
            query.year,
            all_records,
            Timestamp::now(),
        )
    }

    pub async fn purge_user(&self, user_id: UserId) -> DomainResult<()> {
        self.repo.delete_by_user(user_id).await?;
        let mut locks = self.key_locks.lock().await;
        locks.retain(|(uid, _, _), _| *uid != user_id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use jiff::ToSpan;
    use jiff::tz::TimeZone;

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
