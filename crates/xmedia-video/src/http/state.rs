use std::sync::Arc;

use xmedia_core::application::auth::AuthService;
use xmedia_core::application::library::LibraryService;

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
