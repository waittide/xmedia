use uuid::Uuid;
pub(super) use xmedia_source::concurrency::{
    AsyncLockMap, CancelOnDrop, CancelTasksOnDrop, weak_lock,
};
pub(super) use xmedia_source::rate_limit::{RateLimiterMap, get_or_insert_rate_limiter};

pub(super) fn build_image_proxy_url(source_id: Uuid, raw_url: &str) -> String {
    let encoded: String = form_urlencoded::byte_serialize(raw_url.as_bytes()).collect();
    format!("/api/v1/comic/online/image-proxy?source_id={source_id}&url={encoded}")
}
