use async_trait::async_trait;
use toasty::Db;
use uuid::Uuid;
use xmedia_core::domain::error::DomainResult;
use xmedia_core::domain::user::UserId;

use super::db_err;
use crate::domain::online::OnlineShelfBook;
use crate::domain::repository::OnlineShelfRepository;
use crate::infra::persistence::models::BookOnlineShelfModel;

#[derive(Clone)]
pub struct ToastyOnlineShelfRepository {
    db: Db,
}

impl ToastyOnlineShelfRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    fn to_domain(m: BookOnlineShelfModel) -> OnlineShelfBook {
        OnlineShelfBook {
            id: m.id,
            user_id: UserId(m.user_id),
            source_id: m.source_id,
            name: m.name,
            author: m.author,
            book_url: m.book_url,
            toc_url: m.toc_url,
            cover_url: m.cover_url,
            intro: m.intro,
            kind: m.kind,
            last_chapter: m.last_chapter,
            total_chapters: m.total_chapters,
            current_chapter_index: m.current_chapter_index,
            current_chapter_title: m.current_chapter_title,
            scroll_ratio: m.scroll_ratio,
            created_at: m.created_at,
            updated_at: m.updated_at,
        }
    }
}

#[async_trait]
impl OnlineShelfRepository for ToastyOnlineShelfRepository {
    async fn list_by_user(&self, user_id: UserId) -> DomainResult<Vec<OnlineShelfBook>> {
        let mut db = self.db.clone();
        let mut rows = BookOnlineShelfModel::filter_by_user_id(user_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        rows.sort_by_key(|r| std::cmp::Reverse(r.updated_at));
        Ok(rows.into_iter().map(Self::to_domain).collect())
    }

    async fn find_by_id(&self, id: Uuid) -> DomainResult<Option<OnlineShelfBook>> {
        let mut db = self.db.clone();
        let row = BookOnlineShelfModel::filter_by_id(id)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(row.map(Self::to_domain))
    }

    async fn save(&self, item: &OnlineShelfBook) -> DomainResult<()> {
        let mut db = self.db.clone();
        let existing = BookOnlineShelfModel::filter_by_id(item.id)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        if let Some(mut rec) = existing {
            rec.update()
                .source_id(item.source_id)
                .name(item.name.clone())
                .author(item.author.clone())
                .book_url(item.book_url.clone())
                .toc_url(item.toc_url.clone())
                .cover_url(item.cover_url.clone())
                .intro(item.intro.clone())
                .kind(item.kind.clone())
                .last_chapter(item.last_chapter.clone())
                .total_chapters(item.total_chapters)
                .current_chapter_index(item.current_chapter_index)
                .current_chapter_title(item.current_chapter_title.clone())
                .scroll_ratio(item.scroll_ratio)
                .updated_at(item.updated_at)
                .exec(&mut db)
                .await
                .map_err(db_err)?;
        } else {
            toasty::create!(BookOnlineShelfModel {
                id: item.id,
                user_id: item.user_id.0,
                source_id: item.source_id,
                name: item.name.clone(),
                author: item.author.clone(),
                book_url: item.book_url.clone(),
                toc_url: item.toc_url.clone(),
                cover_url: item.cover_url.clone(),
                intro: item.intro.clone(),
                kind: item.kind.clone(),
                last_chapter: item.last_chapter.clone(),
                total_chapters: item.total_chapters,
                current_chapter_index: item.current_chapter_index,
                current_chapter_title: item.current_chapter_title.clone(),
                scroll_ratio: item.scroll_ratio,
                created_at: item.created_at,
                updated_at: item.updated_at,
            })
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        }
        Ok(())
    }

    async fn delete(&self, id: Uuid) -> DomainResult<()> {
        let mut db = self.db.clone();
        BookOnlineShelfModel::filter_by_id(id)
            .delete()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(())
    }

    async fn delete_by_user(&self, user_id: UserId) -> DomainResult<()> {
        let mut db = self.db.clone();
        BookOnlineShelfModel::filter_by_user_id(user_id.0)
            .delete()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(())
    }
}
