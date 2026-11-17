use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use xmedia_core::domain::user::User;
use xmedia_core::http::{
    ApiError, OpdsAuthChallenge, authenticate_opds_parts, authenticate_web_parts,
};

use crate::http::state::ComicHttpState;

pub struct ComicWebAuth(pub User);

impl FromRequestParts<ComicHttpState> for ComicWebAuth {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &ComicHttpState,
    ) -> Result<Self, Self::Rejection> {
        let user = authenticate_web_parts(parts, &state.auth).await?;
        Ok(Self(user))
    }
}

pub struct ComicOpdsAuth(pub User);

impl FromRequestParts<ComicHttpState> for ComicOpdsAuth {
    type Rejection = OpdsAuthChallenge;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &ComicHttpState,
    ) -> Result<Self, Self::Rejection> {
        let user = authenticate_opds_parts(parts, &state.auth).await?;
        Ok(Self(user))
    }
}
