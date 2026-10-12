// SPDX-License-Identifier: GPL-3.0-only

use cosmic::iced;
use futures::SinkExt;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyboardLayouts {
    pub names: Vec<String>,
    pub current_idx: usize,
}

#[derive(Debug, Clone)]
pub enum Event {
    Layouts(KeyboardLayouts),
    Switched(usize),
}

pub fn is_niri() -> bool {
    std::env::var_os("NIRI_SOCKET").is_some()
}

fn parse_layouts(value: &serde_json::Value) -> Option<KeyboardLayouts> {
    let names = value
        .get("names")?
        .as_array()?
        .iter()
        .map(|name| name.as_str().map(str::to_owned))
        .collect::<Option<Vec<_>>>()?;
    let current_idx = value.get("current_idx")?.as_u64()? as usize;
    Some(KeyboardLayouts { names, current_idx })
}

pub async fn switch_layout(idx: usize) -> Result<(), String> {
    let status = Command::new("niri")
        .args(["msg", "action", "switch-layout", &idx.to_string()])
        .status()
        .await
        .map_err(|err| err.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("niri switch-layout exited with {status}"))
    }
}

pub fn subscription() -> iced::Subscription<Event> {
    iced::Subscription::run(event_stream)
}

fn event_stream() -> impl futures::Stream<Item = Event> {
    iced::stream::channel(16, |mut output: futures::channel::mpsc::Sender<Event>| async move {
        loop {
            let Ok(mut child) = Command::new("niri")
                .args(["msg", "--json", "event-stream"])
                .stdout(Stdio::piped())
                .kill_on_drop(true)
                .spawn()
            else {
                tracing::error!("failed to start niri event-stream");
                tokio::time::sleep(Duration::from_secs(2)).await;
                continue;
            };

            if let Some(stdout) = child.stdout.take() {
                let mut lines = BufReader::new(stdout).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
                        continue;
                    };

                    let event = if let Some(layouts) = value
                        .get("KeyboardLayoutsChanged")
                        .and_then(|e| e.get("keyboard_layouts"))
                        .and_then(parse_layouts)
                    {
                        Some(Event::Layouts(layouts))
                    } else {
                        value
                            .get("KeyboardLayoutSwitched")
                            .and_then(|e| e.get("idx"))
                            .and_then(|idx| idx.as_u64())
                            .map(|idx| Event::Switched(idx as usize))
                    };

                    if let Some(event) = event {
                        if output.send(event).await.is_err() {
                            return;
                        }
                    }
                }
            }

            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    })
}
