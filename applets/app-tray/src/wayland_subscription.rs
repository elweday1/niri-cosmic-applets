//! # Compositor subscription for cosmic-app-list
//!
//! Compositor-specific code lives in `wayland_handler`. This module defines the
//! messages exchanged with the rest of the applet.

use cosmic::{
    iced::core::Bytes,
    iced::{self, Subscription, stream},
};
use image::EncodableLayout;

use futures::{
    SinkExt, StreamExt,
    channel::mpsc::{UnboundedReceiver, unbounded},
};
use std::fmt::Debug;
use std::sync::LazyLock;
use tokio::sync::Mutex;

use crate::wayland_handler::wayland_handler;

pub type ToplevelId = u64;
pub type WorkspaceId = u64;

pub static WAYLAND_RX: LazyLock<Mutex<Option<UnboundedReceiver<WaylandUpdate>>>> =
    LazyLock::new(|| Mutex::new(None));

pub fn wayland_subscription() -> iced::Subscription<WaylandUpdate> {
    Subscription::run_with(std::any::TypeId::of::<WaylandUpdate>(), |_| {
        stream::channel(50, move |mut output| async move {
            let mut state = State::Waiting;

            loop {
                state = start_listening(state, &mut output).await;
            }
        })
    })
}

pub enum State {
    Waiting,
    Finished,
}

#[derive(Debug, Clone)]
pub struct WaylandImage {
    pub img: Bytes,
    pub width: u32,
    pub height: u32,
}

impl WaylandImage {
    pub fn new(img: image::RgbaImage) -> Self {
        let width = img.width();
        let height = img.height();

        Self {
            img: Bytes::from_owner(img.into_vec()),
            width,
            height,
        }
    }
}

impl AsRef<[u8]> for WaylandImage {
    fn as_ref(&self) -> &[u8] {
        self.img.as_bytes()
    }
}

async fn start_listening(
    state: State,
    output: &mut futures::channel::mpsc::Sender<WaylandUpdate>,
) -> State {
    match state {
        State::Waiting => {
            let mut guard = WAYLAND_RX.lock().await;
            let rx = {
                if guard.is_none() {
                    let (request_tx, request_rx) = std::sync::mpsc::channel();
                    let (toplevel_tx, toplevel_rx) = unbounded();
                    let _ = std::thread::spawn(move || {
                        wayland_handler(toplevel_tx, request_rx);
                    });
                    *guard = Some(toplevel_rx);
                    _ = output.send(WaylandUpdate::Init(request_tx)).await;
                }
                guard.as_mut().unwrap()
            };
            if let Some(u) = rx.next().await {
                _ = output.send(u).await;
                State::Waiting
            } else {
                _ = output.send(WaylandUpdate::Finished).await;
                tracing::error!("niri handler thread died");
                State::Finished
            }
        }
        State::Finished => iced::futures::future::pending().await,
    }
}

#[derive(Clone, Debug)]
pub enum WaylandUpdate {
    Init(std::sync::mpsc::Sender<WaylandRequest>),
    Finished,
    Toplevel(ToplevelUpdate),
    Workspace(Vec<WorkspaceId>),
    ActivationToken {
        token: Option<String>,
        app_id: Option<String>,
        exec: String,
        gpu_idx: Option<usize>,
        terminal: bool,
    },
    Image(ToplevelId, WaylandImage),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToplevelInfo {
    pub id: ToplevelId,
    pub app_id: String,
    pub title: String,
    pub workspace: Vec<WorkspaceId>,
    pub output: Vec<String>,
    pub activated: bool,
}

#[derive(Clone, Debug)]
pub enum ToplevelUpdate {
    Add(ToplevelInfo),
    Update(ToplevelInfo),
    Remove(ToplevelId),
}

#[derive(Clone, Debug)]
pub enum WaylandRequest {
    Toplevel(ToplevelRequest),
    TokenRequest {
        app_id: String,
        exec: String,
        gpu_idx: Option<usize>,
        terminal: bool,
    },
}

#[derive(Debug, Clone)]
pub enum ToplevelRequest {
    Activate(ToplevelId),
    Minimize(ToplevelId),
    Quit(ToplevelId),
}
