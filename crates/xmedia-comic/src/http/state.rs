use std::sync::Arc;

use xmedia_core::application::auth::AuthService;
use xmedia_core::application::library::LibraryService;

use crate::application::ComicServices;

#[derive(Clone)]
pub struct ComicHttpState {
    pub auth: Arc<AuthService>,
    pub libraries: Arc<LibraryService>,
    pub services: ComicServices,
    pub opds_prefix: Arc<str>,
}

impl ComicHttpState {
    pub fn new(
        auth: Arc<AuthService>,
        libraries: Arc<LibraryService>,
        services: ComicServices,
    ) -> Self {
        Self {
            auth,
            libraries,
            services,
            opds_prefix: Arc::from("/opds/comic"),
        }
    }

    pub fn with_opds_prefix(mut self, prefix: impl Into<Arc<str>>) -> Self {
        self.opds_prefix = prefix.into();
        self
    }
}
