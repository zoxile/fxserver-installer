#[cfg(feature = "desktop")]
use tauri::AppHandle;

use crate::models::jooat::{JooatResolvedHash, JooatResolverStatus};
#[cfg(feature = "desktop")]
use crate::{
    models::jooat::JooatResolverManifest,
    services::jooat::{get_status, prepare_database, remove_database, resolve_hashes, save_shard},
};

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn get_jooat_resolver_status(app: AppHandle) -> Result<JooatResolverStatus, String> {
    super::run_blocking(move || Ok(get_status(&app))).await
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn prepare_jooat_resolver_database(
    app: AppHandle,
    manifest: JooatResolverManifest,
) -> Result<JooatResolverStatus, String> {
    super::run_blocking(move || prepare_database(&app, manifest)).await
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn save_jooat_resolver_shard(
    app: AppHandle,
    prefix: String,
    content: String,
) -> Result<JooatResolverStatus, String> {
    super::run_blocking(move || save_shard(&app, prefix, content)).await
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn remove_jooat_resolver_database(app: AppHandle) -> Result<JooatResolverStatus, String> {
    super::run_blocking(move || remove_database(&app)).await
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn resolve_jooat_hashes(
    app: AppHandle,
    queries: Vec<String>,
) -> Result<Vec<JooatResolvedHash>, String> {
    super::run_blocking(move || resolve_hashes(&app, queries)).await
}

#[cfg(feature = "terminal")]
pub(crate) async fn resolver_status() -> Result<JooatResolverStatus, String> {
    super::run_blocking(|| {
        crate::services::jooat::get_status_in(crate::services::jooat::core_database_dir()?)
    })
    .await
}

#[cfg(feature = "terminal")]
pub(crate) async fn resolve_offline(
    queries: Vec<String>,
) -> Result<Vec<JooatResolvedHash>, String> {
    super::run_blocking(move || {
        crate::services::jooat::resolve_hashes_in(
            crate::services::jooat::core_database_dir()?,
            queries,
        )
    })
    .await
}
