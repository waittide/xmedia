use std::sync::Arc;

use axum::extract::FromRef;
use xmedia_core::application::auth::AuthService;
use xmedia_core::application::library::LibraryService;
use xmedia_core::application::online_items::OnlineLibraryService;

use crate::application::VideoServices;

#[derive(Clone)]
pub struct VideoHttpState {
    pub auth: Arc<AuthService>,
    pub libraries: Arc<LibraryService>,
    pub services: VideoServices,
}

impl VideoHttpState {
    pub fn new(
        auth: Arc<AuthService>,
        libraries: Arc<LibraryService>,
        services: VideoServices,
    ) -> Self {
        Self {
            auth,
            libraries,
            services,
        }
    }
}

impl FromRef<VideoHttpState> for Arc<AuthService> {
    fn from_ref(state: &VideoHttpState) -> Self {
        state.auth.clone()
    }
}

impl FromRef<VideoHttpState> for Arc<OnlineLibraryService> {
    fn from_ref(state: &VideoHttpState) -> Self {
        state.services.online_items.clone()
    }
}
