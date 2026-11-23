pub(crate) mod models;
pub mod repositories;

use std::path::Path;

use toasty::Db;
use toasty::schema::{Model, ModelSet};
use tracing::info;

use crate::domain::error::{DomainError, DomainResult};
use models::{LibraryModel, OnlineLibraryItemModel, UserActivityDailyModel, UserModel};

pub fn register_core_models(set: &mut ModelSet) {
    <UserModel as Model>::register(set);
    <LibraryModel as Model>::register(set);
    <OnlineLibraryItemModel as Model>::register(set);
    <UserActivityDailyModel as Model>::register(set);
}

/// Returns `true` if all core schema tables (`users` and `libraries`) are already present.
pub async fn is_core_schema_initialized(db: &mut Db) -> bool {
    UserModel::all().limit(1).exec(db).await.is_ok()
        && LibraryModel::all().limit(1).exec(db).await.is_ok()
}

async fn connect_bootstrap(set: ModelSet, database_url: &str) -> DomainResult<Db> {
    Db::builder()
        .models(set)
        .connect(database_url)
        .await
        .map_err(|e| DomainError::Persistence(format!("failed to connect core db: {e}")))
}

pub async fn ensure_core_schema(db: &mut Db, database_url: &str) -> DomainResult<()> {
    if !is_core_schema_initialized(db).await {
        info!("initializing core database schema via toasty");
        let mut set = ModelSet::new();
        register_core_models(&mut set);
        let bootstrap = connect_bootstrap(set, database_url).await?;
        bootstrap.push_schema().await.map_err(|e| {
            DomainError::Persistence(format!("failed to push core database schema: {e}"))
        })?;
        return Ok(());
    }
    if OnlineLibraryItemModel::all()
        .limit(1)
        .exec(db)
        .await
        .is_err()
    {
        info!("migrating online_library_items table via toasty");
        let mut set = ModelSet::new();
        <OnlineLibraryItemModel as Model>::register(&mut set);
        let bootstrap = connect_bootstrap(set, database_url).await?;
        bootstrap.push_schema().await.map_err(|e| {
            DomainError::Persistence(format!("failed to push online_library_items schema: {e}"))
        })?;
    }
    if UserActivityDailyModel::all()
        .limit(1)
        .exec(db)
        .await
        .is_err()
    {
        info!("migrating user_activity_daily table via toasty");
        let mut set = ModelSet::new();
        <UserActivityDailyModel as Model>::register(&mut set);
        let bootstrap = connect_bootstrap(set, database_url).await?;
        bootstrap.push_schema().await.map_err(|e| {
            DomainError::Persistence(format!("failed to push user_activity_daily schema: {e}"))
        })?;
    }
    Ok(())
}

pub fn ensure_sqlite_parent_dir(url: &str) -> DomainResult<()> {
    let Some(rest) = url.strip_prefix("sqlite:") else {
        return Ok(());
    };
    if rest.starts_with(":memory:") {
        return Ok(());
    }
    let clean = rest.split('?').next().unwrap_or(rest);
    let path_str = clean.strip_prefix("//").unwrap_or(clean);
    if path_str.is_empty() {
        return Ok(());
    }
    let path = Path::new(path_str);
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(|e| {
            DomainError::Persistence(format!(
                "failed to create SQLite directory {}: {e}",
                parent.display()
            ))
        })?;
    }
    Ok(())
}
