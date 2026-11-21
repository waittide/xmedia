use std::collections::HashMap;

use jiff::Timestamp;
use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::user::{User, UserId};

use crate::domain::online::OnlineShelfBook;

use super::BookOnlineService;
use super::dto::{AddOnlineShelfInput, OnlineShelfBookDto, UpdateOnlineShelfInput};

impl BookOnlineService {
    fn shelf_to_dto(r: OnlineShelfBook, source_name: String) -> OnlineShelfBookDto {
        OnlineShelfBookDto {
            id: r.id,
            source_id: r.source_id,
            source_name,
            name: r.name,
            author: r.author,
            book_url: r.book_url,
            toc_url: r.toc_url,
            cover_url: r.cover_url,
            intro: r.intro,
            kind: r.kind,
            last_chapter: r.last_chapter,
            total_chapters: r.total_chapters,
            current_chapter_index: r.current_chapter_index,
            current_chapter_title: r.current_chapter_title,
            scroll_ratio: r.scroll_ratio,
            updated_at: r.updated_at,
        }
    }

    pub async fn list_shelf(&self, user_id: UserId) -> DomainResult<Vec<OnlineShelfBookDto>> {
        let rows = self.shelves.list_by_user(user_id).await?;
        let sources = self.sources.list_all().await?;
        let src_names: HashMap<Uuid, String> =
            sources.into_iter().map(|s| (s.id, s.name)).collect();

        Ok(rows
            .into_iter()
            .map(|r| {
                let source_name = src_names
                    .get(&r.source_id)
                    .cloned()
                    .unwrap_or_else(|| "已删除书源".to_string());
                Self::shelf_to_dto(r, source_name)
            })
            .collect())
    }

    pub async fn add_to_shelf(
        &self,
        actor: &User,
        input: AddOnlineShelfInput,
    ) -> DomainResult<OnlineShelfBookDto> {
        let src = self.load_source_for_actor(actor, input.source_id).await?;
        let user_id = actor.id;
        let existing_list = self.shelves.list_by_user(user_id).await?;
        let now = Timestamp::now();

        if let Some(mut existing) = existing_list
            .into_iter()
            .find(|b| b.source_id == input.source_id && b.book_url == input.book_url)
        {
            existing.name = input.name;
            existing.author = input.author;
            if !input.toc_url.trim().is_empty() {
                existing.toc_url = input.toc_url;
            }
            if input.cover_url.is_some() {
                existing.cover_url = input.cover_url;
            }
            if input.intro.is_some() {
                existing.intro = input.intro;
            }
            if input.kind.is_some() {
                existing.kind = input.kind;
            }
            if input.last_chapter.is_some() {
                existing.last_chapter = input.last_chapter;
            }
            if let Some(total) = input.total_chapters {
                existing.total_chapters = total;
            }
            existing.updated_at = now;
            self.shelves.save(&existing).await?;
            return Ok(Self::shelf_to_dto(existing, src.name));
        }

        let item = OnlineShelfBook {
            id: Uuid::now_v7(),
            user_id,
            source_id: input.source_id,
            name: input.name,
            author: input.author,
            book_url: input.book_url,
            toc_url: input.toc_url,
            cover_url: input.cover_url,
            intro: input.intro,
            kind: input.kind,
            last_chapter: input.last_chapter,
            total_chapters: input.total_chapters.unwrap_or(0),
            current_chapter_index: 0,
            current_chapter_title: None,
            scroll_ratio: 0.0,
            created_at: now,
            updated_at: now,
        };
        self.shelves.save(&item).await?;
        Ok(Self::shelf_to_dto(item, src.name))
    }

    pub async fn update_shelf_book(
        &self,
        user_id: UserId,
        id: Uuid,
        input: UpdateOnlineShelfInput,
    ) -> DomainResult<OnlineShelfBookDto> {
        let mut rec = self
            .shelves
            .find_by_id(id)
            .await?
            .ok_or_else(|| DomainError::NotFound("未找到在线书架条目".into()))?;
        if rec.user_id != user_id {
            return Err(DomainError::Forbidden("无权操作他人在线书架".into()));
        }

        if let Some(source_id) = input.source_id {
            let src = self.load_source(source_id).await?;
            if !src.enabled {
                return Err(DomainError::Validation("目标书源未启用".into()));
            }
            rec.source_id = source_id;
        }
        if let Some(book_url) = input.book_url {
            rec.book_url = book_url;
        }
        if let Some(toc_url) = input.toc_url {
            rec.toc_url = toc_url;
        }
        if let Some(idx) = input.current_chapter_index {
            rec.current_chapter_index = idx;
        }
        if input.current_chapter_title.is_some() {
            rec.current_chapter_title = input.current_chapter_title;
        }
        if let Some(scroll) = input.scroll_ratio {
            if !scroll.is_finite() {
                return Err(DomainError::Validation("阅读进度比例无效".into()));
            }
            rec.scroll_ratio = scroll.clamp(0.0, 1.0);
        }
        if let Some(total) = input.total_chapters {
            rec.total_chapters = total;
        }
        if input.last_chapter.is_some() {
            rec.last_chapter = input.last_chapter;
        }
        rec.updated_at = Timestamp::now();

        self.shelves.save(&rec).await?;

        let src_name = self
            .load_source(rec.source_id)
            .await
            .map(|s| s.name)
            .unwrap_or_else(|_| "未知书源".to_string());

        Ok(Self::shelf_to_dto(rec, src_name))
    }

    pub async fn remove_from_shelf(&self, user_id: UserId, id: Uuid) -> DomainResult<()> {
        let rec = self
            .shelves
            .find_by_id(id)
            .await?
            .ok_or_else(|| DomainError::NotFound("未找到在线书架条目".into()))?;
        if rec.user_id != user_id {
            return Err(DomainError::Forbidden("无权删除他人在线书架".into()));
        }
        self.shelves.delete(id).await
    }
}
