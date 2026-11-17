use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use xmedia_core::domain::user::User;
use xmedia_core::http::{ApiError, authenticate_web_parts};

use crate::http::state::VideoHttpState;

pub struct VideoWebAuth(pub User);

impl FromRequestParts<VideoHttpState> for VideoWebAuth {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &VideoHttpState,
    ) -> Result<Self, Self::Rejection> {
        let user = authenticate_web_parts(parts, &state.auth).await?;
        Ok(Self(user))
    }
}
