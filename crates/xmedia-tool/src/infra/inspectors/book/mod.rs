mod epub_zip;
mod inspector;
mod opf;
mod sidecar;

pub use inspector::EpubBookInspector;

pub(crate) use epub_zip::{
    attr_value, find_opf_path_in_container, normalize_zip_rel, rewrite_epub_entries,
};
pub(crate) use opf::replace_opf_metadata_block;
pub(crate) use sidecar::{
    dir_has_single_book, find_sidecar_cover, is_generic_book_folder, read_book_meta_any,
};
