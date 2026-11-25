mod filename;
mod inspector;
mod xml;

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) use filename::guess_comic_from_filename;
pub(crate) use filename::{is_generic_comic_folder, is_image_entry, natural_sort_key};
pub use inspector::CbzComicInspector;
pub(crate) use inspector::read_comic_meta;
pub(crate) use xml::{
    extract_existing_comic_info_bytes, render_comic_info_xml_preserving,
    render_comic_info_xml_with_pages,
};
