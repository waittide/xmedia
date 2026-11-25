use std::path::Path;

pub(super) use xmedia_formats::comic::filename::find_series_cover_path;
pub(crate) use xmedia_formats::comic::filename::{is_generic_comic_folder, natural_page_entry_cmp};
pub(crate) use xmedia_formats::comic::is_comic_image_entry as is_image_entry;

pub(super) fn guess_comic_details(
    comic_path: &Path,
) -> (Option<String>, Option<String>, Option<f64>, Option<String>) {
    xmedia_formats::comic::filename::guess_comic_details_from_path(comic_path)
}

#[cfg(test)]
pub(crate) fn guess_comic_from_filename(
    comic_path: &Path,
) -> (Option<String>, Option<String>, Option<f64>) {
    let (series, number, volume, _) = guess_comic_details(comic_path);
    (series, number, volume)
}
