use std::sync::Arc;

use axum::extract::FromRef;
use xmedia_core::application::auth::AuthService;
use xmedia_core::application::library::LibraryService;
use xmedia_core::application::online_items::OnlineLibraryService;

use crate::application::BookServices;
use crate::application::online::BookOnlineSourceDirectory;

#[derive(Clone)]
pub struct BookHttpState {
    pub auth: Arc<AuthService>,
    pub libraries: Arc<LibraryService>,
    pub services: BookServices,
    pub opds_prefix: Arc<str>,
}

impl FromRef<BookHttpState> for Arc<AuthService> {
    fn from_ref(input: &BookHttpState) -> Self {
        input.auth.clone()
    }
}

impl FromRef<BookHttpState> for Arc<OnlineLibraryService> {
    fn from_ref(input: &BookHttpState) -> Self {
        input.services.online_items.clone()
    }
}

impl FromRef<BookHttpState> for Arc<BookOnlineSourceDirectory> {
    fn from_ref(input: &BookHttpState) -> Self {
        input.services.sources.clone()
    }
}

impl BookHttpState {
    pub fn new(
        auth: Arc<AuthService>,
        libraries: Arc<LibraryService>,
        services: BookServices,
    ) -> Self {
        Self {
            auth,
            libraries,
            services,
            opds_prefix: Arc::from("/opds/book"),
        }
    }

    pub fn with_opds_prefix(mut self, prefix: impl Into<Arc<str>>) -> Self {
        self.opds_prefix = prefix.into();
        self
    }
}
