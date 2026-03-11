use std::{
    io::Read,
    path::{Path, PathBuf},
    sync::OnceLock,
};

use anyhow::Context;
use chrono::Utc;
use futures::StreamExt;
use orchestrator_core::ProviderKind;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sysinfo::System;
use uuid::Uuid;

use crate::{
    config::Config,
    db::{CreateModelInstallJobInput, Database, LlamaCppModel, ModelInstallJob, UpdateModelInstallJobInput},
    process::run_provider_diagnostics,
    state::AppState,
};

const BROWSER_SOURCE_KEY: &str = "latest";

static CATALOG: OnceLock<Vec<CuratedModelCatalogEntry>> = OnceLock::new();

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DeviceHardwareProfile {
    pub source: String,
    pub platform: Option<String>,
    pub cpu_brand: Option<String>,
    pub logical_cores: Option<u32>,
    pub total_memory_gb: Option<f32>,
    pub available_memory_gb: Option<f32>,
    pub device_memory_gb: Option<f32>,
    pub gpu_vendor: Option<String>,
    pub gpu_renderer: Option<String>,
    pub user_agent: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct ProvidersHardwareView {
    pub authority: String,
    pub host: DeviceHardwareProfile,
    pub browser: Option<DeviceHardwareProfile>,
}

#[derive(Debug, Serialize, Clone)]
pub struct GovernanceProviderStatus {
    pub provider: ProviderKind,
    pub display_name: String,
    pub installed: bool,
    pub auth_status: String,
    pub detail: Option<String>,
    pub issues: Vec<String>,
    pub version: Option<String>,
    pub status_label: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct ProvidersGovernanceView {
    pub authority: String,
    pub ollama_api_base: String,
    pub ollama_runtime_mode: String,
    pub host: DeviceHardwareProfile,
    pub browser: Option<DeviceHardwareProfile>,
    pub local_providers: Vec<GovernanceProviderStatus>,
    pub inventory_issue_count: usize,
    pub inventory_issues: Vec<String>,
    pub recent_jobs: Vec<ModelInstallJob>,
    pub last_refresh_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BrowserHardwareSnapshotInput {
    pub platform: Option<String>,
    pub cpu_brand: Option<String>,
    pub logical_cores: Option<u32>,
    pub device_memory_gb: Option<f32>,
    pub total_memory_gb: Option<f32>,
    pub available_memory_gb: Option<f32>,
    pub gpu_vendor: Option<String>,
    pub gpu_renderer: Option<String>,
    pub user_agent: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CuratedModelCatalogEntry {
    pub key: String,
    pub runtime_target: String,
    pub model_ref: String,
    pub display_name: String,
    pub family: String,
    pub summary: String,
    pub objectives: Vec<String>,
    pub modality: String,
    pub parameter_size_b: f32,
    pub artifact_size_gb: f32,
    pub context_window: i32,
    pub quantization: Option<String>,
    pub memory_min_gb: f32,
    pub memory_recommended_gb: f32,
    pub install_policy: String,
    pub benchmark_coding: Option<i32>,
    pub benchmark_reasoning: Option<i32>,
    pub benchmark_vision: Option<i32>,
    pub source_label: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct LocalModelCatalogItem {
    #[serde(flatten)]
    pub entry: CuratedModelCatalogEntry,
    pub policy_state: String,
    pub recommendation_band: String,
    pub fit_reason: String,
    pub installed: bool,
}

#[derive(Debug, Serialize, Clone)]
pub struct InstalledLocalModel {
    pub runtime_target: String,
    pub model_ref: String,
    pub display_name: String,
    pub alias: Option<String>,
    pub enabled: bool,
    pub context_window: Option<i32>,
    pub quantization: Option<String>,
    pub file_path: Option<String>,
    pub installed_from_catalog: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct LocalModelInventoryView {
    pub ollama: Vec<InstalledLocalModel>,
    pub gguf: Vec<InstalledLocalModel>,
    pub issues: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct CatalogQuery {
    pub objective: Option<String>,
    pub runtime: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct InstallCatalogModelRequest {
    pub catalog_key: String,
    pub actor_name: Option<String>,
    pub source_app: Option<String>,
    pub source_channel: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ImportGgufRequest {
    pub alias: String,
    pub file_path: String,
    pub context_window: Option<i32>,
    pub quantization: Option<String>,
    pub actor_name: Option<String>,
    pub source_app: Option<String>,
    pub source_channel: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct GgufImportResponse {
    pub model: LlamaCppModel,
    pub job: ModelInstallJob,
}

pub fn curated_catalog() -> &'static [CuratedModelCatalogEntry] {
    CATALOG.get_or_init(|| {
        serde_json::from_str(include_str!("local_model_catalog.json"))
            .expect("local model catalog must be valid JSON")
    })
}

pub fn find_catalog_entry(key: &str) -> Option<CuratedModelCatalogEntry> {
    curated_catalog().iter().find(|entry| entry.key == key).cloned()
}

pub async fn current_hardware_view(db: &Database) -> anyhow::Result<ProvidersHardwareView> {
    let host = host_hardware_profile();
    let browser = db
        .get_hardware_profile("browser", BROWSER_SOURCE_KEY)
        .await?
        .map(|record| serde_json::from_value(record.payload))
        .transpose()
        .context("invalid browser hardware payload")?;

    Ok(ProvidersHardwareView {
        authority: "host".to_string(),
        host,
        browser,
    })
}

pub async fn providers_governance_view(state: &AppState) -> anyhow::Result<ProvidersGovernanceView> {
    let hardware = current_hardware_view(&state.db).await?;
    let inventory = local_inventory(state).await?;
    let recent_jobs = state.db.list_model_install_jobs(6).await?;
    let last_refresh_at = Utc::now().to_rfc3339();
    let mut local_providers = Vec::new();

    for provider in [ProviderKind::Ollama, ProviderKind::LlamaCpp] {
        if let Some(adapter) = state.provider(provider) {
            let diagnostic = run_provider_diagnostics(&adapter).await;
            let issues = match provider {
                ProviderKind::LlamaCpp if inventory.gguf.iter().all(|model| !model.enabled) => {
                    let mut issues = diagnostic.issues.clone();
                    issues.push("Register at least one enabled GGUF model for llama.cpp.".to_string());
                    issues
                }
                _ => diagnostic.issues.clone(),
            };
            let status_label = if matches!(
                diagnostic.status,
                orchestrator_core::ProviderInstallStatus::Missing
            ) {
                "missing".to_string()
            } else if !issues.is_empty() || matches!(diagnostic.auth_status, orchestrator_core::AuthStatus::Error | orchestrator_core::AuthStatus::MissingDependency) {
                "attention".to_string()
            } else {
                "healthy".to_string()
            };

            local_providers.push(GovernanceProviderStatus {
                provider,
                display_name: provider.display_name().to_string(),
                installed: matches!(
                    diagnostic.status,
                    orchestrator_core::ProviderInstallStatus::Installed
                ),
                auth_status: format!("{:?}", diagnostic.auth_status).to_ascii_lowercase(),
                detail: diagnostic.detail,
                issues,
                version: diagnostic.version,
                status_label,
            });
        }
    }

    Ok(ProvidersGovernanceView {
        authority: hardware.authority,
        ollama_api_base: state.config.ollama_api_base.clone(),
        ollama_runtime_mode: infer_ollama_runtime_mode(&state.config),
        host: hardware.host,
        browser: hardware.browser,
        local_providers,
        inventory_issue_count: inventory.issues.len(),
        inventory_issues: inventory.issues,
        recent_jobs,
        last_refresh_at,
    })
}

pub async fn persist_browser_snapshot(
    db: &Database,
    snapshot: BrowserHardwareSnapshotInput,
) -> anyhow::Result<DeviceHardwareProfile> {
    let profile = DeviceHardwareProfile {
        source: "browser".to_string(),
        platform: snapshot.platform,
        cpu_brand: snapshot.cpu_brand,
        logical_cores: snapshot.logical_cores,
        total_memory_gb: snapshot.total_memory_gb,
        available_memory_gb: snapshot.available_memory_gb,
        device_memory_gb: snapshot.device_memory_gb,
        gpu_vendor: snapshot.gpu_vendor,
        gpu_renderer: snapshot.gpu_renderer,
        user_agent: snapshot.user_agent,
        updated_at: Utc::now().to_rfc3339(),
    };
    let payload = serde_json::to_value(&profile)?;
    db.upsert_hardware_profile("browser", BROWSER_SOURCE_KEY, &payload)
        .await?;
    Ok(profile)
}

pub async fn local_inventory(state: &AppState) -> anyhow::Result<LocalModelInventoryView> {
    let gguf_models = state.db.list_llama_cpp_models().await?;
    let mut issues = Vec::new();
    let ollama = match list_installed_ollama_models(&state.config.ollama_api_base).await {
        Ok(models) => models,
        Err(error) => {
            issues.push(format!("Ollama inventory unavailable: {error}"));
            Vec::new()
        }
    };

    let gguf = gguf_models
        .into_iter()
        .map(|model| {
            let installed_from_catalog = catalog_key_for_gguf_model(&model);
            InstalledLocalModel {
                runtime_target: "llama_cpp".to_string(),
                model_ref: model.file_path.clone(),
                display_name: model.alias.clone(),
                alias: Some(model.alias),
                enabled: model.enabled,
                context_window: model.context_window,
                quantization: model.quantization,
                file_path: Some(model.file_path),
                installed_from_catalog,
            }
        })
        .collect();

    Ok(LocalModelInventoryView {
        ollama,
        gguf,
        issues,
    })
}

pub async fn catalog_view(
    db: &Database,
    state: &AppState,
    query: &CatalogQuery,
) -> anyhow::Result<Vec<LocalModelCatalogItem>> {
    let hardware = current_hardware_view(db).await?;
    let inventory = local_inventory(state).await?;
    let installed = inventory
        .ollama
        .iter()
        .map(|item| item.model_ref.clone())
        .chain(
            inventory
                .gguf
                .iter()
                .filter_map(|item| item.installed_from_catalog.clone()),
        )
        .collect::<std::collections::HashSet<_>>();

    let objective = query.objective.as_deref().map(str::trim).filter(|value| !value.is_empty());
    let runtime = query.runtime.as_deref().map(str::trim).filter(|value| !value.is_empty());

    let mut items = curated_catalog()
        .iter()
        .filter(|entry| {
            objective
                .map(|goal| entry.objectives.iter().any(|item| item == goal))
                .unwrap_or(true)
                && runtime.map(|value| entry.runtime_target == value).unwrap_or(true)
        })
        .map(|entry| {
            let installed_key_match = installed.contains(&entry.key);
            let installed_ref_match = installed.contains(&entry.model_ref);
            let installed = installed_key_match || installed_ref_match;
            let (recommendation_band, fit_reason) = score_catalog_entry(entry, &hardware.host);
            let policy_state = if installed {
                "already_installed".to_string()
            } else if entry.install_policy == "approved" {
                "approved_for_install".to_string()
            } else {
                "visible_but_blocked".to_string()
            };
            LocalModelCatalogItem {
                entry: entry.clone(),
                policy_state,
                recommendation_band,
                fit_reason,
                installed,
            }
        })
        .collect::<Vec<_>>();

    items.sort_by(|left, right| {
        rank_recommendation_band(&left.recommendation_band)
            .cmp(&rank_recommendation_band(&right.recommendation_band))
            .then_with(|| left.entry.display_name.cmp(&right.entry.display_name))
    });
    Ok(items)
}

pub async fn start_ollama_catalog_install(
    state: AppState,
    request: InstallCatalogModelRequest,
) -> anyhow::Result<ModelInstallJob> {
    let entry = find_catalog_entry(request.catalog_key.trim())
        .ok_or_else(|| anyhow::anyhow!("catalog entry {} not found", request.catalog_key.trim()))?;
    if entry.runtime_target != "ollama" {
        anyhow::bail!("catalog entry {} is not an Ollama model", entry.key);
    }
    if entry.install_policy != "approved" {
        anyhow::bail!("catalog entry {} is blocked by policy", entry.key);
    }

    let job = state
        .db
        .create_model_install_job(CreateModelInstallJobInput {
            actor_name: request
                .actor_name
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or("local-operator")
                .to_string(),
            source_app: request
                .source_app
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or("ferrum-web")
                .to_string(),
            source_channel: request
                .source_channel
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or("providers")
                .to_string(),
            runtime_target: "ollama".to_string(),
            catalog_key: Some(entry.key.clone()),
            source_ref: Some(entry.model_ref.clone()),
            checksum_expected: None,
            status: "pending".to_string(),
            progress_percent: 0,
            detail: Some("Queued approved Ollama install".to_string()),
        })
        .await?;

    let state_for_task = state.clone();
    let entry_for_task = entry.clone();
    tokio::spawn(async move {
        if let Err(error) = run_ollama_install(&state_for_task, job.id, &entry_for_task).await {
            let _ = state_for_task
                .db
                .update_model_install_job(
                    job.id,
                    UpdateModelInstallJobInput {
                        status: "failed".to_string(),
                        progress_percent: 0,
                        detail: Some("Ollama install failed".to_string()),
                        checksum_actual: None,
                        destination_ref: None,
                        error_text: Some(error.to_string()),
                    },
                )
                .await;
        }
    });

    Ok(job)
}

pub async fn import_gguf_model(
    state: &AppState,
    request: ImportGgufRequest,
) -> anyhow::Result<GgufImportResponse> {
    let alias = request.alias.trim();
    let file_path = request.file_path.trim();
    if alias.is_empty() || file_path.is_empty() {
        anyhow::bail!("alias and file_path are required");
    }

    let resolved = resolve_import_path(&state.config.llama_cpp_model_dir, file_path);
    if resolved
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.eq_ignore_ascii_case("gguf"))
        != Some(true)
    {
        anyhow::bail!("only .gguf files can be imported");
    }

    let exists = tokio::fs::try_exists(&resolved)
        .await
        .with_context(|| format!("failed to access {}", resolved.display()))?;
    if !exists {
        anyhow::bail!("gguf file not found at {}", resolved.display());
    }

    let checksum = checksum_file_sha256(&resolved).await?;
    let model = state
        .db
        .upsert_llama_cpp_model(
            alias,
            &resolved.to_string_lossy(),
            request.context_window,
            request.quantization.as_deref(),
            true,
        )
        .await?;
    let job = state
        .db
        .create_model_install_job(CreateModelInstallJobInput {
            actor_name: request
                .actor_name
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or("local-operator")
                .to_string(),
            source_app: request
                .source_app
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or("ferrum-web")
                .to_string(),
            source_channel: request
                .source_channel
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or("providers")
                .to_string(),
            runtime_target: "llama_cpp".to_string(),
            catalog_key: catalog_key_for_gguf_path(&resolved),
            source_ref: Some(resolved.to_string_lossy().to_string()),
            checksum_expected: Some(checksum.clone()),
            status: "completed".to_string(),
            progress_percent: 100,
            detail: Some("Imported local GGUF into controlled registry".to_string()),
        })
        .await?;
    let job = state
        .db
        .update_model_install_job(
            job.id,
            UpdateModelInstallJobInput {
                status: "completed".to_string(),
                progress_percent: 100,
                detail: Some("Imported local GGUF into controlled registry".to_string()),
                checksum_actual: Some(checksum),
                destination_ref: Some(resolved.to_string_lossy().to_string()),
                error_text: None,
            },
        )
        .await?
        .ok_or_else(|| anyhow::anyhow!("import job missing after update"))?;

    Ok(GgufImportResponse { model, job })
}

async fn run_ollama_install(
    state: &AppState,
    job_id: Uuid,
    entry: &CuratedModelCatalogEntry,
) -> anyhow::Result<()> {
    state
        .db
        .update_model_install_job(
            job_id,
            UpdateModelInstallJobInput {
                status: "running".to_string(),
                progress_percent: 5,
                detail: Some(format!("Requesting Ollama pull for {}", entry.model_ref)),
                checksum_actual: None,
                destination_ref: None,
                error_text: None,
            },
        )
        .await?;

    let client = Client::builder().build()?;
    let response = client
        .post(format!("{}/api/pull", state.config.ollama_api_base))
        .json(&serde_json::json!({
            "model": entry.model_ref,
            "stream": true
        }))
        .send()
        .await
        .with_context(|| format!("failed to reach Ollama at {}", state.config.ollama_api_base))?;

    if !response.status().is_success() {
        anyhow::bail!("Ollama pull returned HTTP {}", response.status());
    }

    let mut stream = response.bytes_stream();
    let mut buffer = String::new();
    let mut last_progress = 5;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        buffer.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(index) = buffer.find('\n') {
            let line = buffer[..index].trim().to_string();
            buffer = buffer[index + 1..].to_string();
            if line.is_empty() {
                continue;
            }
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) {
                let detail = value
                    .get("status")
                    .and_then(|item| item.as_str())
                    .map(|item| item.to_string())
                    .unwrap_or_else(|| "pulling approved Ollama model".to_string());
                let progress = progress_from_ollama_event(&value).unwrap_or(last_progress);
                last_progress = progress.max(last_progress);
                state
                    .db
                    .update_model_install_job(
                        job_id,
                        UpdateModelInstallJobInput {
                            status: "running".to_string(),
                            progress_percent: last_progress.min(95),
                            detail: Some(detail),
                            checksum_actual: None,
                            destination_ref: Some(entry.model_ref.clone()),
                            error_text: None,
                        },
                    )
                    .await?;
            }
        }
    }

    state
        .db
        .update_model_install_job(
            job_id,
            UpdateModelInstallJobInput {
                status: "completed".to_string(),
                progress_percent: 100,
                detail: Some(format!("Installed approved Ollama model {}", entry.model_ref)),
                checksum_actual: None,
                destination_ref: Some(entry.model_ref.clone()),
                error_text: None,
            },
        )
        .await?;
    Ok(())
}

fn host_hardware_profile() -> DeviceHardwareProfile {
    let mut system = System::new_all();
    system.refresh_all();

    let cpu_brand = system
        .cpus()
        .first()
        .map(|cpu| cpu.brand().trim().to_string())
        .filter(|value| !value.is_empty());
    let total_memory_gb = bytes_to_gb(system.total_memory());
    let available_memory_gb = bytes_to_gb(system.available_memory());

    DeviceHardwareProfile {
        source: "host".to_string(),
        platform: Some(format!("{} {}", System::name().unwrap_or_default(), System::os_version().unwrap_or_default()).trim().to_string())
            .filter(|value| !value.is_empty()),
        cpu_brand,
        logical_cores: Some(system.cpus().len() as u32),
        total_memory_gb: Some(total_memory_gb),
        available_memory_gb: Some(available_memory_gb),
        device_memory_gb: Some(total_memory_gb),
        gpu_vendor: None,
        gpu_renderer: None,
        user_agent: None,
        updated_at: Utc::now().to_rfc3339(),
    }
}

async fn list_installed_ollama_models(base_url: &str) -> anyhow::Result<Vec<InstalledLocalModel>> {
    let client = Client::builder().build()?;
    let response = client
        .get(format!("{base_url}/api/tags"))
        .send()
        .await
        .with_context(|| format!("failed to reach Ollama at {base_url}"))?;
    if !response.status().is_success() {
        anyhow::bail!("Ollama inventory returned HTTP {}", response.status());
    }

    let payload = response.json::<OllamaTagsResponse>().await?;
    Ok(payload
        .models
        .into_iter()
        .map(|model| InstalledLocalModel {
            runtime_target: "ollama".to_string(),
            model_ref: model.name.clone(),
            display_name: model.name.clone(),
            alias: None,
            enabled: true,
            context_window: None,
            quantization: model.details.and_then(|details| details.quantization_level),
            file_path: None,
            installed_from_catalog: catalog_key_for_ollama_model(&model.name),
        })
        .collect())
}

fn score_catalog_entry(
    entry: &CuratedModelCatalogEntry,
    hardware: &DeviceHardwareProfile,
) -> (String, String) {
    let host_memory = hardware.total_memory_gb.unwrap_or(0.0);
    if entry.install_policy != "approved" {
        return (
            "visible_but_blocked".to_string(),
            "Visible for planning but blocked by enterprise policy.".to_string(),
        );
    }
    if host_memory >= entry.memory_recommended_gb {
        return (
            "recommended".to_string(),
            format!(
                "Host memory {:.1} GB meets recommended footprint {:.1} GB.",
                host_memory, entry.memory_recommended_gb
            ),
        );
    }
    if host_memory >= entry.memory_min_gb {
        return (
            "possible_with_tradeoffs".to_string(),
            format!(
                "Host memory {:.1} GB clears minimum {:.1} GB but not the recommended {:.1} GB.",
                host_memory, entry.memory_min_gb, entry.memory_recommended_gb
            ),
        );
    }
    (
        "not_recommended".to_string(),
        format!(
            "Host memory {:.1} GB is below the minimum {:.1} GB footprint.",
            host_memory, entry.memory_min_gb
        ),
    )
}

fn rank_recommendation_band(value: &str) -> i32 {
    match value {
        "recommended" => 0,
        "possible_with_tradeoffs" => 1,
        "visible_but_blocked" => 2,
        _ => 3,
    }
}

fn resolve_import_path(base_dir: &Path, raw: &str) -> PathBuf {
    let candidate = PathBuf::from(raw);
    if candidate.is_absolute() {
        candidate
    } else {
        base_dir.join(candidate)
    }
}

async fn checksum_file_sha256(path: &Path) -> anyhow::Result<String> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let mut file = std::fs::File::open(&path)
            .with_context(|| format!("failed to open {}", path.display()))?;
        let mut hasher = Sha256::new();
        let mut buffer = [0_u8; 8192];
        loop {
            let read = file.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
        }
        Ok::<String, anyhow::Error>(format!("{:x}", hasher.finalize()))
    })
    .await
    .context("checksum task join failed")?
}

fn progress_from_ollama_event(value: &serde_json::Value) -> Option<i32> {
    let completed = value.get("completed")?.as_f64()?;
    let total = value.get("total")?.as_f64()?;
    if total <= 0.0 {
        return None;
    }
    Some(((completed / total) * 100.0).round() as i32)
}

fn bytes_to_gb(bytes: u64) -> f32 {
    (bytes as f64 / 1024_f64 / 1024_f64 / 1024_f64) as f32
}

fn infer_ollama_runtime_mode(config: &Config) -> String {
    let Some(url) = reqwest::Url::parse(&config.ollama_api_base).ok() else {
        return "endpoint_only".to_string();
    };
    let Some(host) = url.host_str().map(|value| value.to_ascii_lowercase()) else {
        return "endpoint_only".to_string();
    };

    if matches!(host.as_str(), "127.0.0.1" | "localhost" | "::1") {
        "host".to_string()
    } else if host == "ollama" || host.contains("docker") {
        "docker".to_string()
    } else {
        "endpoint_only".to_string()
    }
}

fn catalog_key_for_ollama_model(model_ref: &str) -> Option<String> {
    curated_catalog()
        .iter()
        .find(|entry| entry.runtime_target == "ollama" && entry.model_ref == model_ref)
        .map(|entry| entry.key.clone())
}

fn catalog_key_for_gguf_path(path: &Path) -> Option<String> {
    let file_name = path.file_name()?.to_string_lossy();
    curated_catalog()
        .iter()
        .find(|entry| entry.runtime_target == "llama_cpp" && entry.model_ref == file_name)
        .map(|entry| entry.key.clone())
}

fn catalog_key_for_gguf_model(model: &LlamaCppModel) -> Option<String> {
    catalog_key_for_gguf_path(Path::new(&model.file_path))
}

#[derive(Debug, Deserialize)]
struct OllamaTagsResponse {
    #[serde(default)]
    models: Vec<OllamaTagModel>,
}

#[derive(Debug, Deserialize)]
struct OllamaTagModel {
    name: String,
    details: Option<OllamaTagDetails>,
}

#[derive(Debug, Deserialize)]
struct OllamaTagDetails {
    quantization_level: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::{bytes_to_gb, curated_catalog, infer_ollama_runtime_mode, resolve_import_path, score_catalog_entry};
    use crate::config::Config;

    fn test_config(base_url: &str) -> Config {
        Config {
            bind_addr: "127.0.0.1:3000".parse().expect("bind addr"),
            database_url: "postgres://chatbot:chatbot@127.0.0.1:5433/chatbot".to_string(),
            workspace_dir: ".".into(),
            frontend_dir: "apps/web-lab/dist".into(),
            llama_cpp_model_dir: "var/models/llama.cpp".into(),
            ollama_api_base: base_url.to_string(),
            codex_daily_soft_limit: None,
            claude_daily_soft_limit: None,
        }
    }

    #[test]
    fn infers_host_runtime_mode_for_local_ollama() {
        assert_eq!(infer_ollama_runtime_mode(&test_config("http://127.0.0.1:11434")), "host");
        assert_eq!(infer_ollama_runtime_mode(&test_config("http://localhost:11434")), "host");
    }

    #[test]
    fn infers_docker_runtime_mode_for_service_name() {
        assert_eq!(infer_ollama_runtime_mode(&test_config("http://ollama:11434")), "docker");
    }

    #[test]
    fn falls_back_to_endpoint_only_for_unknown_remote() {
        assert_eq!(
            infer_ollama_runtime_mode(&test_config("http://lan-host.internal:11434")),
            "endpoint_only"
        );
    }

    #[test]
    fn curated_catalog_contains_approved_entries_for_both_local_runtimes() {
        let has_ollama = curated_catalog().iter().any(|entry| entry.runtime_target == "ollama");
        let has_llama_cpp = curated_catalog().iter().any(|entry| entry.runtime_target == "llama_cpp");
        assert!(has_ollama);
        assert!(has_llama_cpp);
    }

    #[test]
    fn memory_scoring_prefers_recommended_fit() {
        let entry = curated_catalog()
            .iter()
            .find(|entry| entry.runtime_target == "ollama" && entry.install_policy == "approved")
            .expect("approved ollama entry");
        let hardware = super::DeviceHardwareProfile {
            source: "host".to_string(),
            platform: None,
            cpu_brand: None,
            logical_cores: None,
            total_memory_gb: Some(entry.memory_recommended_gb + 1.0),
            available_memory_gb: Some(entry.memory_recommended_gb + 0.5),
            device_memory_gb: Some(entry.memory_recommended_gb + 1.0),
            gpu_vendor: None,
            gpu_renderer: None,
            user_agent: None,
            updated_at: "now".to_string(),
        };
        let (band, _) = score_catalog_entry(entry, &hardware);
        assert_eq!(band, "recommended");
    }

    #[test]
    fn resolve_import_path_joins_relative_paths() {
        let path = resolve_import_path(std::path::Path::new("var/models/llama.cpp"), "my-model.gguf");
        assert_eq!(
            path,
            std::path::PathBuf::from("var/models/llama.cpp").join("my-model.gguf")
        );
    }

    #[test]
    fn bytes_to_gb_is_reasonable() {
        assert_eq!(bytes_to_gb(1024 * 1024 * 1024), 1.0);
    }
}
