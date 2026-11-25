use std::sync::Arc;

use axum::extract::FromRef;
use xmedia_core::application::auth::AuthService;
use xmedia_core::application::library::LibraryService;
use xmedia_core::application::online_items::OnlineLibraryService;
use xmedia_source::crud::SourceCrudService;

use crate::application::ComicServices;
use crate::domain::online::ComicSourceSpec;

#[derive(Clone)]
pub struct ComicHttpState {
    pub auth: Arc<AuthService>,
    pub libraries: Arc<LibraryService>,
    pub services: ComicServices,
    pub opds_prefix: Arc<str>,
}

impl FromRef<ComicHttpState> for Arc<AuthService> {
    fn from_ref(state: &ComicHttpState) -> Self {
        state.auth.clone()
    }
}

impl FromRef<ComicHttpState> for Arc<OnlineLibraryService> {
    fn from_ref(state: &ComicHttpState) -> Self {
        state.services.online_items.clone()
    }
}

impl FromRef<ComicHttpState> for Arc<SourceCrudService<ComicSourceSpec>> {
    fn from_ref(state: &ComicHttpState) -> Self {
        state.services.online.source_crud().clone()
    }
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
