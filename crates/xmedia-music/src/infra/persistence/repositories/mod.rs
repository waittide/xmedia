mod catalog;
mod collections;
mod sources;

use jiff::Timestamp;
use uuid::Uuid;
use xmedia_core::domain::error::DomainError;

pub use catalog::ToastyMusicCatalogRepository;
pub use collections::ToastyMusicCollectionRepository;
pub use sources::ToastyMusicSourceRepository;

pub(super) fn db_err(err: impl std::fmt::Display) -> DomainError {
    DomainError::Persistence(err.to_string())
}

pub(super) fn shuffle_in_place<T>(slice: &mut [T]) {
    if slice.len() <= 1 {
        return;
    }
    let mut state = (Uuid::now_v7().as_u128() as u64)
        ^ (Timestamp::now().as_nanosecond() as u64)
        ^ 0x9E37_79B9_7F4A_7C15;
    for i in (1..slice.len()).rev() {
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z = z ^ (z >> 31);
        let j = (z as usize) % (i + 1);
        slice.swap(i, j);
    }
}
