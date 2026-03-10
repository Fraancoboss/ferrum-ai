use std::{collections::HashMap, sync::Arc};

use orchestrator_core::{NormalizedEvent, ProviderAdapter, ProviderKind};
use tokio::sync::{RwLock, broadcast};
use uuid::Uuid;

use crate::{config::Config, db::Database};

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub db: Database,
    pub providers: Arc<HashMap<ProviderKind, ProviderAdapter>>,
    pub hub: Arc<EventHub>,
    pub provider_prefs: Arc<RwLock<HashMap<ProviderKind, ProviderPreferences>>>,
}

impl AppState {
    pub fn provider(&self, provider: ProviderKind) -> Option<ProviderAdapter> {
        self.providers.get(&provider).cloned()
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct ProviderPreferences {
    pub model: Option<String>,
    pub effort: Option<String>,
}

pub fn default_provider_prefs() -> HashMap<ProviderKind, ProviderPreferences> {
    HashMap::from([
        (
            ProviderKind::Codex,
            ProviderPreferences {
                model: Some("gpt-5-codex".to_string()),
                effort: Some("high".to_string()),
            },
        ),
        (
            ProviderKind::Claude,
            ProviderPreferences {
                model: Some("sonnet".to_string()),
                effort: Some("medium".to_string()),
            },
        ),
    ])
}

#[derive(Default)]
pub struct EventHub {
    runs: RwLock<HashMap<Uuid, broadcast::Sender<NormalizedEvent>>>,
    auth: RwLock<HashMap<Uuid, broadcast::Sender<NormalizedEvent>>>,
}

impl EventHub {
    pub async fn ensure_run_sender(&self, run_id: Uuid) -> broadcast::Sender<NormalizedEvent> {
        self.ensure_sender(&self.runs, run_id).await
    }

    pub async fn ensure_auth_sender(&self, auth_id: Uuid) -> broadcast::Sender<NormalizedEvent> {
        self.ensure_sender(&self.auth, auth_id).await
    }

    pub async fn subscribe_run(&self, run_id: Uuid) -> broadcast::Receiver<NormalizedEvent> {
        self.ensure_run_sender(run_id).await.subscribe()
    }

    pub async fn subscribe_auth(&self, auth_id: Uuid) -> broadcast::Receiver<NormalizedEvent> {
        self.ensure_auth_sender(auth_id).await.subscribe()
    }

    pub async fn publish_run(&self, run_id: Uuid, event: NormalizedEvent) {
        let sender = self.ensure_run_sender(run_id).await;
        let _ = sender.send(event);
    }

    pub async fn publish_auth(&self, auth_id: Uuid, event: NormalizedEvent) {
        let sender = self.ensure_auth_sender(auth_id).await;
        let _ = sender.send(event);
    }

    async fn ensure_sender(
        &self,
        store: &RwLock<HashMap<Uuid, broadcast::Sender<NormalizedEvent>>>,
        id: Uuid,
    ) -> broadcast::Sender<NormalizedEvent> {
        {
            let read = store.read().await;
            if let Some(existing) = read.get(&id) {
                return existing.clone();
            }
        }

        let mut write = store.write().await;
        write
            .entry(id)
            .or_insert_with(|| broadcast::channel(256).0)
            .clone()
    }
}
