use std::sync::Arc;

use xmedia_core::application::auth::AuthService;
use xmedia_core::application::library::LibraryService;

use crate::application::MusicServices;

#[derive(Clone)]
pub struct MusicHttpState {
    pub auth: Arc<AuthService>,
    pub libraries: Arc<LibraryService>,
    pub services: MusicServices,
}

impl MusicHttpState {
    pub fn new(
        auth: Arc<AuthService>,
        libraries: Arc<LibraryService>,
        services: MusicServices,
    ) -> Self {
        Self {
            auth,
            libraries,
            services,
        }
    }
}
