use tauri::AppHandle;

use crate::{
    models::jooat::{JooatResolvedHash, JooatResolverManifest, JooatResolverStatus},
    services::jooat::{get_status, prepare_database, remove_database, resolve_hashes, save_shard},
};

#[tauri::command]
pub async fn get_jooat_resolver_status(app: AppHandle) -> Result<JooatResolverStatus, String> {
    super::run_blocking(move || Ok(get_status(&app))).await
}

#[tauri::command]
pub async fn prepare_jooat_resolver_database(
    app: AppHandle,
    manifest: JooatResolverManifest,
) -> Result<JooatResolverStatus, String> {
    super::run_blocking(move || prepare_database(&app, manifest)).await
}

#[tauri::command]
pub async fn save_jooat_resolver_shard(
    app: AppHandle,
    prefix: String,
    content: String,
) -> Result<JooatResolverStatus, String> {
    super::run_blocking(move || save_shard(&app, prefix, content)).await
}

#[tauri::command]
pub async fn remove_jooat_resolver_database(app: AppHandle) -> Result<JooatResolverStatus, String> {
    super::run_blocking(move || remove_database(&app)).await
}

#[tauri::command]
pub async fn resolve_jooat_hashes(
    app: AppHandle,
    queries: Vec<String>,
) -> Result<Vec<JooatResolvedHash>, String> {
    super::run_blocking(move || resolve_hashes(&app, queries)).await
}
