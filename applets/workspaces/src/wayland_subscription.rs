// Copyright 2023 System76 <info@system76.com>
// SPDX-License-Identifier: GPL-3.0-only

//! niri backend for cosmic-applet-workspaces.
//!
//! niri does not expose the ext-workspace protocol, so workspace state comes from
//! niri's IPC event stream.

use cosmic::iced::{
    self, Subscription,
    futures::{SinkExt, StreamExt, channel::mpsc},
    stream,
};
use niri_ipc::{Event, Request, Workspace, socket::Socket};
use std::io;

#[derive(Debug, Clone)]
pub enum WorkspacesUpdate {
    Workspaces(Vec<Workspace>),
    Errored,
}

pub fn workspaces() -> iced::Subscription<WorkspacesUpdate> {
    Subscription::run_with(std::any::TypeId::of::<WorkspacesUpdate>(), |_| {
        stream::channel(50, move |mut output: mpsc::Sender<WorkspacesUpdate>| async move {
            let (tx, mut rx) = mpsc::unbounded::<WorkspacesUpdate>();
            std::thread::spawn(move || {
                if let Err(err) = listen(&tx) {
                    tracing::error!("niri event stream stopped: {err}");
                }
                let _ = tx.unbounded_send(WorkspacesUpdate::Errored);
            });
            while let Some(update) = rx.next().await {
                let errored = matches!(update, WorkspacesUpdate::Errored);
                if output.send(update).await.is_err() || errored {
                    break;
                }
            }
            cosmic::iced::futures::future::pending().await
        })
    })
}

fn listen(tx: &mpsc::UnboundedSender<WorkspacesUpdate>) -> io::Result<()> {
    let mut socket = Socket::connect()?;
    socket
        .send(Request::EventStream)?
        .map_err(io::Error::other)?;

    let mut next_event = socket.read_events();
    let mut workspaces: Vec<Workspace> = Vec::new();
    loop {
        match next_event()? {
            Event::WorkspacesChanged { workspaces: list } => workspaces = list,
            Event::WorkspaceActivated { id, focused } => {
                let output = workspaces
                    .iter()
                    .find(|w| w.id == id)
                    .and_then(|w| w.output.clone());
                for ws in &mut workspaces {
                    if ws.output == output {
                        ws.is_active = ws.id == id;
                    }
                    if focused {
                        ws.is_focused = ws.id == id;
                    }
                }
            }
            Event::WorkspaceUrgencyChanged { id, urgent } => {
                if let Some(ws) = workspaces.iter_mut().find(|w| w.id == id) {
                    ws.is_urgent = urgent;
                }
            }
            _ => continue,
        }
        if tx
            .unbounded_send(WorkspacesUpdate::Workspaces(workspaces.clone()))
            .is_err()
        {
            return Ok(());
        }
    }
}
