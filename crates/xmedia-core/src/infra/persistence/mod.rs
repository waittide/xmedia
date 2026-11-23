pub(crate) mod models;
pub mod repositories;

use std::path::Path;

use toasty::Db;
use toasty::schema::{Model, ModelSet};
use tracing::info;

use crate::domain::error::{DomainError, DomainResult};
use models::{
    LibraryModel, OnlineLibraryItemModel, UserActivityDailyModel, UserMediaBindingModel, UserModel,
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CoreSchemaMigrationReport {
    pub bindings_table_created: bool,
}

pub fn register_core_models(set: &mut ModelSet) {
    <UserModel as Model>::register(set);
    <LibraryModel as Model>::register(set);
    <OnlineLibraryItemModel as Model>::register(set);
    <UserActivityDailyModel as Model>::register(set);
    <UserMediaBindingModel as Model>::register(set);
}

/// Returns `true` if all core schema tables (`users` and `libraries`) are already present.
pub async fn is_core_schema_initialized(db: &mut Db) -> bool {
    UserModel::all().limit(1).exec(db).await.is_ok()
        && LibraryModel::all().limit(1).exec(db).await.is_ok()
}

/// Connects once with the supplied `ModelSet` and pushes all missing tables in a single pass.
pub async fn push_model_set(database_url: &str, label: &str, set: ModelSet) -> DomainResult<()> {
    let bootstrap = Db::builder()
        .models(set)
        .connect(database_url)
        .await
        .map_err(|e| DomainError::Persistence(format!("failed to connect {label} db: {e}")))?;
    bootstrap
        .push_schema()
        .await
        .map_err(|e| DomainError::Persistence(format!("failed to push {label} schema: {e}")))?;
    Ok(())
}

pub async fn ensure_core_schema(db: &mut Db, database_url: &str) -> DomainResult<()> {
    let _ = ensure_core_schema_with_report(db, database_url).await?;
    Ok(())
}

pub async fn ensure_core_schema_with_report(
    db: &mut Db,
    database_url: &str,
) -> DomainResult<CoreSchemaMigrationReport> {
    if !is_core_schema_initialized(db).await {
        info!("initializing core database schema via toasty");
        let mut set = ModelSet::new();
        register_core_models(&mut set);
        push_model_set(database_url, "core database", set).await?;
        return Ok(CoreSchemaMigrationReport {
            bindings_table_created: false,
        });
    }

    let mut report = CoreSchemaMigrationReport::default();
    let mut missing = ModelSet::new();
    let mut labels = Vec::new();
    if OnlineLibraryItemModel::all()
        .limit(1)
        .exec(db)
        .await
        .is_err()
    {
        <OnlineLibraryItemModel as Model>::register(&mut missing);
        labels.push("online_library_items");
    }
    if UserActivityDailyModel::all()
        .limit(1)
        .exec(db)
        .await
        .is_err()
    {
        <UserActivityDailyModel as Model>::register(&mut missing);
        labels.push("user_activity_daily");
    }
    if UserMediaBindingModel::all()
        .limit(1)
        .exec(db)
        .await
        .is_err()
    {
        <UserMediaBindingModel as Model>::register(&mut missing);
        labels.push("user_media_bindings");
        report.bindings_table_created = true;
    }
    if !labels.is_empty() {
        let joined = labels.join(", ");
        info!("migrating core tables ({joined}) via toasty");
        push_model_set(database_url, &joined, missing).await?;
    }
    Ok(report)
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
