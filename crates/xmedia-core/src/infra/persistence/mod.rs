pub(crate) mod models;
pub mod repositories;

use std::path::Path;

use toasty::Db;
use toasty::schema::{Model, ModelSet};
use tracing::info;

use crate::domain::error::{DomainError, DomainResult};
use models::{
    LibraryModel, OnlineLibraryItemModel, UserActivityDailyModel, UserMediaBindingModel, UserModel,
    UserPluginModel,
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CoreSchemaMigrationReport {
    pub bindings_table_created: bool,
    pub plugins_table_created: bool,
}

pub fn register_core_models(set: &mut ModelSet) {
    <UserModel as Model>::register(set);
    <LibraryModel as Model>::register(set);
    <OnlineLibraryItemModel as Model>::register(set);
    <UserActivityDailyModel as Model>::register(set);
    <UserMediaBindingModel as Model>::register(set);
    <UserPluginModel as Model>::register(set);
}

pub async fn is_core_schema_initialized(db: &mut Db) -> bool {
    UserModel::all().limit(1).exec(db).await.is_ok()
        && LibraryModel::all().limit(1).exec(db).await.is_ok()
}

fn is_mysql_duplicate_index(msg: &str) -> bool {
    msg.contains("1061") || msg.to_ascii_lowercase().contains("duplicate key name")
}

/// Ensures a secondary SQL index exists across SQLite, PostgreSQL, MariaDB, and MySQL.
pub async fn ensure_sql_index(
    db: &mut Db,
    index_name: &str,
    table_and_cols: &str,
) -> DomainResult<()> {
    if matches!(db.capability().sql, Some(toasty::Dialect::Mysql)) {
        let ddl = format!("CREATE INDEX {index_name} ON {table_and_cols}");
        if let Err(err) = toasty::sql::statement(ddl).exec(db).await {
            let msg = err.to_string();
            if !is_mysql_duplicate_index(&msg) {
                return Err(DomainError::Persistence(format!(
                    "failed to ensure index {index_name}: {err}"
                )));
            }
        }
        return Ok(());
    }
    let ddl = format!("CREATE INDEX IF NOT EXISTS {index_name} ON {table_and_cols}");
    toasty::sql::statement(ddl).exec(db).await.map_err(|e| {
        DomainError::Persistence(format!("failed to ensure index {index_name}: {e}"))
    })?;
    Ok(())
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

async fn collect_missing_core_tables(
    db: &mut Db,
) -> (ModelSet, Vec<&'static str>, CoreSchemaMigrationReport) {
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
    if UserPluginModel::all().limit(1).exec(db).await.is_err() {
        <UserPluginModel as Model>::register(&mut missing);
        labels.push("user_plugins");
        report.plugins_table_created = true;
    }
    (missing, labels, report)
}

pub async fn ensure_core_schema(
    db: &mut Db,
    database_url: &str,
) -> DomainResult<CoreSchemaMigrationReport> {
    if !is_core_schema_initialized(db).await {
        info!("initializing core database schema via toasty");
        let mut set = ModelSet::new();
        register_core_models(&mut set);
        push_model_set(database_url, "core database", set).await?;
        return Ok(CoreSchemaMigrationReport::default());
    }

    let (missing, labels, report) = collect_missing_core_tables(db).await;
    if !labels.is_empty() {
        let joined = labels.join(", ");
        info!("migrating core tables ({joined}) via toasty");
        push_model_set(database_url, &joined, missing).await?;
    }
    Ok(report)
}

fn sqlite_db_path(url: &str) -> Option<&Path> {
    let rest = url.strip_prefix("sqlite:")?;
    if rest.starts_with(":memory:") {
        return None;
    }
    let clean = rest.split('?').next().unwrap_or(rest);
    let path_str = clean.strip_prefix("//").unwrap_or(clean);
    if path_str.is_empty() {
        None
    } else {
        Some(Path::new(path_str))
    }
}

pub fn ensure_sqlite_parent_dir(url: &str) -> DomainResult<()> {
    if let Some(parent) = sqlite_db_path(url).and_then(Path::parent)
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(|e| {
            DomainError::Storage(format!(
                "failed to create sqlite parent directory '{}': {e}",
                parent.display()
            ))
        })?;
    }
    Ok(())
}
