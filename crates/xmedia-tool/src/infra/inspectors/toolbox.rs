use xmedia_core::domain::error::DomainResult;

use crate::application::ops::{ToolboxCommand, ToolboxOutput};
use crate::application::ports::MediaToolboxPort;

use super::{book_ops, comic_ops, music_ops, video_ops};

#[derive(Debug, Default, Clone, Copy)]
pub struct DefaultMediaToolbox;

impl MediaToolboxPort for DefaultMediaToolbox {
    fn execute(&self, cmd: ToolboxCommand) -> DomainResult<ToolboxOutput> {
        match cmd {
            ToolboxCommand::Book(c) => book_ops::run_book_tool(c).map(ToolboxOutput::Book),
            ToolboxCommand::Comic(c) => comic_ops::run_comic_tool(c).map(ToolboxOutput::Comic),
            ToolboxCommand::Music(c) => music_ops::run_music_tool(c).map(ToolboxOutput::Music),
            ToolboxCommand::Video(c) => video_ops::run_video_tool(c).map(ToolboxOutput::Video),
        }
    }
}
