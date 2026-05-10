use tauri::AppHandle;

use crate::{
    models::jooat::{JooatResolvedHash, JooatResolverManifest, JooatResolverStatus},
    services::jooat::{get_status, prepare_database, remove_database, resolve_hashes, save_shard},
};

#[tauri::command]
pub fn get_jooat_resolver_status(app: AppHandle) -> JooatResolverStatus {
    get_status(&app)
}

#[tauri::command]
pub fn prepare_jooat_resolver_database(
    app: AppHandle,
    manifest: JooatResolverManifest,
) -> Result<JooatResolverStatus, String> {
    prepare_database(&app, manifest)
}

#[tauri::command]
pub fn save_jooat_resolver_shard(
    app: AppHandle,
    prefix: String,
    content: String,
) -> Result<JooatResolverStatus, String> {
    save_shard(&app, prefix, content)
}

#[tauri::command]
pub fn remove_jooat_resolver_database(app: AppHandle) -> Result<JooatResolverStatus, String> {
    remove_database(&app)
}

#[tauri::command]
pub fn resolve_jooat_hashes(
    app: AppHandle,
    queries: Vec<String>,
) -> Result<Vec<JooatResolvedHash>, String> {
    resolve_hashes(&app, queries)
}
