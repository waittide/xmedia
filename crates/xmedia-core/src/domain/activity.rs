use async_trait::async_trait;
use jiff::Timestamp;
use uuid::Uuid;

use crate::domain::error::DomainResult;
use crate::domain::library::MediaKind;
use crate::domain::user::UserId;

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
