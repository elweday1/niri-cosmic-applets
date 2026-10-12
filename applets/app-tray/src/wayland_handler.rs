// Copyright 2023 System76 <info@system76.com>
// SPDX-License-Identifier: GPL-3.0-only

//! niri backend for cosmic-app-list.
//!
//! niri does not expose the cosmic toplevel protocols, so window state comes from
//! niri's IPC event stream and actions are sent over the same IPC socket.

use crate::wayland_subscription::{
    ToplevelId, ToplevelInfo, ToplevelRequest, ToplevelUpdate, WaylandRequest, WaylandUpdate,
    WorkspaceId,
};
use futures::channel::mpsc::UnboundedSender;
use niri_ipc::{Action, Event, Request, Window, Workspace, socket::Socket};
use std::{
    collections::HashMap,
    io,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, RecvTimeoutError},
    },
    time::Duration,
};

pub(crate) fn wayland_handler(
    tx: UnboundedSender<WaylandUpdate>,
    rx: Receiver<WaylandRequest>,
) {
    let listening = Arc::new(AtomicBool::new(true));

    let listener_tx = tx.clone();
    let listener_flag = listening.clone();
    std::thread::spawn(move || {
        if let Err(err) = listen(&listener_tx) {
            tracing::error!("niri event stream stopped: {err}");
        }
        listener_flag.store(false, Ordering::SeqCst);
    });

    loop {
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(req) => handle_request(&tx, req),
            Err(RecvTimeoutError::Timeout) => {
                if !listening.load(Ordering::SeqCst) {
                    break;
                }
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
}

fn handle_request(tx: &UnboundedSender<WaylandUpdate>, req: WaylandRequest) {
    match req {
        WaylandRequest::Toplevel(req) => match req {
            ToplevelRequest::Activate(id) => send_action(Action::FocusWindow { id }),
            ToplevelRequest::Quit(id) => send_action(Action::CloseWindow { id: Some(id) }),
            ToplevelRequest::Minimize(_) => {
                tracing::debug!("minimize is not supported by niri");
            }
        },
        WaylandRequest::TokenRequest {
            app_id,
            exec,
            gpu_idx,
            terminal,
        } => {
            // niri does not implement xdg-activation tokens for IPC launches.
            let _ = tx.unbounded_send(WaylandUpdate::ActivationToken {
                token: None,
                app_id: Some(app_id),
                exec,
                gpu_idx,
                terminal,
            });
        }
    }
}

fn send_action(action: Action) {
    let result = Socket::connect()
        .and_then(|mut socket| socket.send(Request::Action(action)))
        .and_then(|reply| reply.map_err(io::Error::other));
    if let Err(err) = result {
        tracing::error!("niri action failed: {err}");
    }
}

fn listen(tx: &UnboundedSender<WaylandUpdate>) -> io::Result<()> {
    let mut socket = Socket::connect()?;
    socket
        .send(Request::EventStream)?
        .map_err(io::Error::other)?;

    let mut next_event = socket.read_events();
    let mut state = NiriState::default();
    while !tx.is_closed() {
        state.apply(next_event()?);
        state.sync(tx);
    }
    Ok(())
}

#[derive(Default)]
struct NiriState {
    windows: Vec<Window>,
    workspaces: Vec<Workspace>,
    sent_toplevels: HashMap<ToplevelId, ToplevelInfo>,
    sent_workspaces: Vec<WorkspaceId>,
}

impl NiriState {
    fn apply(&mut self, event: Event) {
        match event {
            Event::WorkspacesChanged { workspaces } => self.workspaces = workspaces,
            Event::WorkspaceActivated { id, .. } => {
                let output = self
                    .workspaces
                    .iter()
                    .find(|w| w.id == id)
                    .and_then(|w| w.output.clone());
                for ws in &mut self.workspaces {
                    if ws.output == output {
                        ws.is_active = ws.id == id;
                    }
                }
            }
            Event::WindowsChanged { windows } => self.windows = windows,
            Event::WindowOpenedOrChanged { window } => {
                match self.windows.iter_mut().find(|w| w.id == window.id) {
                    Some(existing) => *existing = window,
                    None => self.windows.push(window),
                }
            }
            Event::WindowClosed { id } => self.windows.retain(|w| w.id != id),
            Event::WindowFocusChanged { id } => {
                for w in &mut self.windows {
                    w.is_focused = Some(w.id) == id;
                }
            }
            _ => {}
        }
    }

    fn output_of(&self, workspace_id: WorkspaceId) -> Option<String> {
        self.workspaces
            .iter()
            .find(|w| w.id == workspace_id)
            .and_then(|w| w.output.clone())
    }

    fn sync(&mut self, tx: &UnboundedSender<WaylandUpdate>) {
        let active_workspaces: Vec<WorkspaceId> = self
            .workspaces
            .iter()
            .filter(|w| w.is_active)
            .map(|w| w.id)
            .collect();
        if active_workspaces != self.sent_workspaces {
            self.sent_workspaces = active_workspaces.clone();
            let _ = tx.unbounded_send(WaylandUpdate::Workspace(active_workspaces));
        }

        let desired: Vec<ToplevelInfo> = self
            .windows
            .iter()
            .map(|w| ToplevelInfo {
                id: w.id,
                app_id: w.app_id.clone().unwrap_or_default(),
                title: w.title.clone().unwrap_or_default(),
                workspace: w.workspace_id.into_iter().collect(),
                output: w
                    .workspace_id
                    .and_then(|ws| self.output_of(ws))
                    .into_iter()
                    .collect(),
                activated: w.is_focused,
            })
            .collect();

        let removed: Vec<ToplevelId> = self
            .sent_toplevels
            .keys()
            .filter(|id| !desired.iter().any(|info| info.id == **id))
            .copied()
            .collect();
        for id in removed {
            self.sent_toplevels.remove(&id);
            let _ = tx.unbounded_send(WaylandUpdate::Toplevel(ToplevelUpdate::Remove(id)));
        }

        for info in desired {
            match self.sent_toplevels.get(&info.id) {
                Some(sent) if *sent == info => {}
                Some(_) => {
                    self.sent_toplevels.insert(info.id, info.clone());
                    let _ = tx.unbounded_send(WaylandUpdate::Toplevel(ToplevelUpdate::Update(info)));
                }
                None => {
                    self.sent_toplevels.insert(info.id, info.clone());
                    let _ = tx.unbounded_send(WaylandUpdate::Toplevel(ToplevelUpdate::Add(info)));
                }
            }
        }
    }
}
