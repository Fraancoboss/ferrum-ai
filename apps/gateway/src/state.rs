use std::{collections::HashMap, sync::Arc};

use orchestrator_core::{NormalizedEvent, ProviderAdapter, ProviderKind};
use tokio::sync::{RwLock, broadcast};
use uuid::Uuid;

use crate::{
    config::Config,
    db::{Database, TerminalOutput},
};

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
        (
            ProviderKind::Ollama,
            ProviderPreferences {
                model: Some("llama3.1:8b".to_string()),
                effort: None,
            },
        ),
        (
            ProviderKind::LlamaCpp,
            ProviderPreferences {
                model: Some("var/models/llama.cpp/model.gguf".to_string()),
                effort: None,
            },
        ),
    ])
}

#[derive(Default)]
pub struct EventHub {
    runs: RwLock<HashMap<Uuid, broadcast::Sender<NormalizedEvent>>>,
    auth: RwLock<HashMap<Uuid, broadcast::Sender<NormalizedEvent>>>,
    terminals: RwLock<HashMap<Uuid, broadcast::Sender<TerminalOutput>>>,
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

    pub async fn ensure_terminal_sender(
        &self,
        terminal_id: Uuid,
    ) -> broadcast::Sender<TerminalOutput> {
        self.ensure_terminal_output_sender(&self.terminals, terminal_id)
            .await
    }

    pub async fn subscribe_terminal(
        &self,
        terminal_id: Uuid,
    ) -> broadcast::Receiver<TerminalOutput> {
        self.ensure_terminal_sender(terminal_id).await.subscribe()
    }

    pub async fn publish_terminal(&self, terminal_id: Uuid, chunk: TerminalOutput) {
        let sender = self.ensure_terminal_sender(terminal_id).await;
        let _ = sender.send(chunk);
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

    async fn ensure_terminal_output_sender(
        &self,
        store: &RwLock<HashMap<Uuid, broadcast::Sender<TerminalOutput>>>,
        id: Uuid,
    ) -> broadcast::Sender<TerminalOutput> {
        {
            let read = store.read().await;
            if let Some(existing) = read.get(&id) {
                return existing.clone();
            }
        }

        let mut write = store.write().await;
        write
            .entry(id)
            .or_insert_with(|| broadcast::channel(512).0)
            .clone()
    }
}
