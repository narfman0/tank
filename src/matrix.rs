use anyhow::{Context, Result};
use matrix_sdk::{
    config::SyncSettings,
    ruma::events::room::message::{
        MessageType, OriginalSyncRoomMessageEvent, RoomMessageEventContent,
    },
    ruma::RoomId,
    Client, Room,
};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::mpsc;

use crate::config::{Config, Session};

pub struct MatrixVoiceClient {
    client: Client,
    config: Arc<Config>,
}

impl MatrixVoiceClient {
    pub async fn new(config: Arc<Config>, store_path: &Path) -> Result<Self> {
        let client = Client::builder()
            .homeserver_url(&config.matrix.homeserver)
            .sqlite_store(store_path, None)
            .build()
            .await
            .context("failed to build matrix client")?;

        Ok(Self { client, config })
    }

    /// Restore or perform fresh login. Returns the client ready for use.
    pub async fn authenticate(&self, config_dir: &Path) -> Result<()> {
        // Try to restore from saved session file first
        if let Some(session) = self.config.load_session(config_dir) {
            tracing::info!("restoring saved matrix session for {}", session.user_id);
            match self.restore_session(&session).await {
                Ok(()) => {
                    tracing::info!("session restored successfully");
                    return Ok(());
                }
                Err(e) => {
                    tracing::warn!("session restore failed ({}), falling back to password login", e);
                }
            }
        }

        // Fresh password login
        tracing::info!("performing password login as {}", self.config.matrix.username);
        self.client
            .matrix_auth()
            .login_username(&self.config.matrix.username, &self.config.matrix.password)
            .initial_device_display_name("matrixvoice")
            .send()
            .await
            .context("password login failed")?;

        // Persist session for next run
        if let Some(session) = self.current_session() {
            self.config.save_session(config_dir, &session)?;
            tracing::info!("session saved to {}", self.config.matrix.session_file);
        }

        Ok(())
    }

    async fn restore_session(&self, session: &Session) -> Result<()> {
        use matrix_sdk::matrix_auth::MatrixSession;
        use matrix_sdk::SessionMeta;
        use matrix_sdk::ruma::OwnedUserId;

        let matrix_session = MatrixSession {
            meta: SessionMeta {
                user_id: session.user_id.parse::<OwnedUserId>()?,
                device_id: session.device_id.as_str().into(),
            },
            tokens: matrix_sdk::matrix_auth::MatrixSessionTokens {
                access_token: session.access_token.clone(),
                refresh_token: None,
            },
        };
        self.client.restore_session(matrix_session).await?;
        Ok(())
    }

    fn current_session(&self) -> Option<Session> {
        let s = self.client.matrix_auth().session()?;
        Some(Session {
            access_token: s.tokens.access_token.clone(),
            device_id: s.meta.device_id.to_string(),
            user_id: s.meta.user_id.to_string(),
            homeserver: self.config.matrix.homeserver.clone(),
        })
    }

    /// Spawn a background sync task. Messages received in listen rooms are forwarded to `tx`.
    pub async fn start_sync(&self, tx: mpsc::Sender<String>) -> Result<()> {
        let listen_rooms: Vec<String> = self
            .config
            .matrix
            .rooms
            .iter()
            .filter(|r| r.listen)
            .map(|r| r.id.clone())
            .collect();

        let tx_clone = tx.clone();
        self.client.add_event_handler(
            move |ev: OriginalSyncRoomMessageEvent, room: Room| {
                let tx = tx_clone.clone();
                let listen_rooms = listen_rooms.clone();
                async move {
                    let room_id = room.room_id().to_string();
                    if !listen_rooms.contains(&room_id) {
                        return;
                    }
                    if let MessageType::Text(text_content) = ev.content.msgtype {
                        tracing::debug!("received message from {}: {}", ev.sender, text_content.body);
                        let _ = tx.send(text_content.body).await;
                    }
                }
            },
        );

        let client = self.client.clone();
        tokio::spawn(async move {
            tracing::info!("starting matrix sync loop");
            if let Err(e) = client.sync(SyncSettings::default()).await {
                tracing::error!("sync loop error: {}", e);
            }
        });

        Ok(())
    }

    /// Send a text message to all configured send rooms.
    pub async fn send_message(&self, text: &str) -> Result<()> {
        let send_rooms: Vec<String> = self
            .config
            .matrix
            .rooms
            .iter()
            .filter(|r| r.send)
            .map(|r| r.id.clone())
            .collect();

        for room_id_str in &send_rooms {
            let room_id: &RoomId = room_id_str.as_str().try_into()?;
            if let Some(room) = self.client.get_room(room_id) {
                let content = RoomMessageEventContent::text_plain(text);
                room.send(content).await?;
                tracing::info!("sent to {}: {}", room_id_str, text);
            } else {
                tracing::warn!("not a member of room {}", room_id_str);
            }
        }
        Ok(())
    }
}
