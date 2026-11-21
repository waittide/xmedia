mod artwork;
mod filename;
mod inspector;
mod nfo;

pub use inspector::VideoNfoInspector;

pub(crate) use artwork::write_image_replacing_opposite_ext;
pub(crate) use filename::{dir_has_single_video, is_season_dir};
pub(crate) use nfo::{
    apply_video_patch_onto, read_base_video_meta, render_season_nfo_xml, render_tvshow_nfo_xml,
    render_video_nfo_xml,
};
