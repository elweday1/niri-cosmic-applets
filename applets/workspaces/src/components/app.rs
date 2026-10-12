// Copyright 2023 System76 <info@system76.com>
// SPDX-License-Identifier: GPL-3.0-only

use niri_ipc::{Action, Workspace, WorkspaceReferenceArg, socket::Socket};
use cosmic::{
    Element, Task, Theme, app,
    applet::cosmic_panel_config::PanelAnchor,
    iced::core::{Background, Border},
    iced::{
        Alignment,
        Event::Mouse,
        Length, Limits, Subscription, event,
        mouse::{self, ScrollDelta},
        widget::{button, column, row},
    },
    scroll::DiscreteScrollState,
    widget::{Id, autosize, container},
};

use crate::{
    config,
    wayland_subscription::{WorkspacesUpdate, workspaces},
};

use std::{io, process::Command as ShellCommand, sync::LazyLock, time::Duration};

static AUTOSIZE_MAIN_ID: LazyLock<Id> = LazyLock::new(|| Id::new("autosize-main"));

const SCROLL_RATE_LIMIT: Duration = Duration::from_millis(200);

pub fn run() -> cosmic::iced::Result {
    cosmic::applet::run::<IcedWorkspacesApplet>(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    Row,
    Column,
}

struct IcedWorkspacesApplet {
    core: cosmic::app::Core,
    workspaces: Vec<Workspace>,
    layout: Layout,
    scroll: DiscreteScrollState,
}

impl IcedWorkspacesApplet {
    /// returns the index of the workspace button after which which must be moved to a popup
    /// if it exists.
    fn popup_index(&self) -> Option<usize> {
        let mut index = None;
        let Some(max_major_axis_len) = self.core.applet.suggested_bounds.as_ref().map(|c| {
            // if we have a configure for width and height, we're in a overflow popup
            match self.core.applet.anchor {
                PanelAnchor::Top | PanelAnchor::Bottom => c.width as u32,
                PanelAnchor::Left | PanelAnchor::Right => c.height as u32,
            }
        }) else {
            return index;
        };
        let button_total_size = self.core.applet.suggested_size(true).0
            + self.core.applet.suggested_padding(true).1 * 2
            + 4;
        let btn_count = max_major_axis_len / button_total_size as u32;
        if btn_count >= self.workspaces.len() as u32 {
            index = None;
        } else {
            index = Some((btn_count as usize).min(self.workspaces.len()));
        }
        index
    }
}

#[derive(Debug, Clone)]
enum Message {
    WorkspaceUpdate(WorkspacesUpdate),
    WorkspacePressed(u64),
    WheelScrolled(ScrollDelta),
    WorkspaceOverview,
}

impl cosmic::Application for IcedWorkspacesApplet {
    type Message = Message;
    type Executor = cosmic::SingleThreadExecutor;
    type Flags = ();
    const APP_ID: &'static str = config::APP_ID;

    fn init(core: cosmic::app::Core, _flags: Self::Flags) -> (Self, app::Task<Self::Message>) {
        (
            Self {
                layout: match &core.applet.anchor {
                    PanelAnchor::Left | PanelAnchor::Right => Layout::Column,
                    PanelAnchor::Top | PanelAnchor::Bottom => Layout::Row,
                },
                core,
                workspaces: Vec::new(),
                scroll: DiscreteScrollState::default().rate_limit(Some(SCROLL_RATE_LIMIT)),
            },
            Task::none(),
        )
    }

    fn core(&self) -> &cosmic::app::Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut cosmic::app::Core {
        &mut self.core
    }

    fn update(&mut self, message: Self::Message) -> app::Task<Self::Message> {
        match message {
            Message::WorkspaceUpdate(msg) => match msg {
                WorkspacesUpdate::Workspaces(list) => {
                    self.workspaces = visible_workspaces(list);
                }
                WorkspacesUpdate::Errored => {
                    tracing::error!("niri workspace stream stopped");
                }
            },
            Message::WorkspacePressed(id) => send_action(Action::FocusWorkspace {
                reference: WorkspaceReferenceArg::Id(id),
            }),
            Message::WheelScrolled(delta) => {
                let discrete_delta = self.scroll.update(delta);
                if discrete_delta.y > 0 {
                    send_action(Action::FocusWorkspaceUp {});
                } else if discrete_delta.y < 0 {
                    send_action(Action::FocusWorkspaceDown {});
                }
            }
            Message::WorkspaceOverview => {
                let _ = ShellCommand::new("cosmic-workspaces").spawn();
            }
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        if self.workspaces.is_empty() {
            return row![].padding(8).into();
        }
        let horizontal = matches!(
            self.core.applet.anchor,
            PanelAnchor::Top | PanelAnchor::Bottom
        );
        let suggested_total = self.core.applet.suggested_size(true).0
            + self.core.applet.suggested_padding(true).1 * 2;
        let popup_index = self.popup_index().unwrap_or(self.workspaces.len());

        let buttons = self.workspaces[..popup_index].iter().map(|w| {
            let label = roman(w.idx);
            let side = suggested_total as f32 * 0.7;
            let text_class = if w.is_active {
                cosmic::theme::Text::Custom(active_numeral)
            } else {
                cosmic::theme::Text::Default
            };
            let content = container(cosmic::widget::text(label).size(12.0).class(text_class))
                .width(Length::Fixed(side * 1.3))
                .height(Length::Fixed(side * 0.9))
                .align_x(Alignment::Center)
                .align_y(Alignment::Center);

            let btn = button(content)
                .padding(if horizontal {
                    [0, self.core.applet.suggested_padding(true).1]
                } else {
                    [self.core.applet.suggested_padding(true).1, 0]
                })
                .on_press(
                    if w.is_active {
                        Message::WorkspaceOverview
                    } else {
                        Message::WorkspacePressed(w.id)
                    },
                )
                .padding(0);

            btn.class(
                if w.is_active {
                    cosmic::theme::iced::Button::Primary
                } else if w.is_urgent {
                    let appearance = |theme: &Theme| {
                        let cosmic = theme.cosmic();
                        button::Style {
                            background: Some(Background::Color(cosmic.palette.neutral_3.into())),
                            border: Border {
                                radius: cosmic.radius_xl().into(),
                                ..Default::default()
                            },
                            border_radius: theme.cosmic().radius_xl().into(),
                            text_color: theme.cosmic().destructive_button.base.into(),
                            ..button::Style::default()
                        }
                    };
                    cosmic::theme::iced::Button::Custom(Box::new(
                        move |theme, status| match status {
                            button::Status::Active => appearance(theme),
                            button::Status::Hovered => button::Style {
                                background: Some(Background::Color(
                                    theme.current_container().component.hover.into(),
                                )),
                                border: Border {
                                    radius: theme.cosmic().radius_xl().into(),
                                    ..Default::default()
                                },
                                ..appearance(theme)
                            },
                            button::Status::Pressed => appearance(theme),
                            button::Status::Disabled => appearance(theme),
                        },
                    ))
                } else {
                    let appearance = |theme: &Theme| {
                        let cosmic = theme.cosmic();
                        button::Style {
                            background: Some(Background::Color(
                                theme.current_container().component.hover.into(),
                            )),
                            border: Border {
                                radius: cosmic.radius_xl().into(),
                                ..Default::default()
                            },
                            border_radius: cosmic.radius_xl().into(),
                            text_color: theme.current_container().component.on.into(),
                            ..button::Style::default()
                        }
                    };
                    cosmic::theme::iced::Button::Custom(Box::new(
                        move |theme, status| match status {
                            button::Status::Active => appearance(theme),
                            button::Status::Hovered => button::Style {
                                background: Some(Background::Color(
                                    theme.current_container().component.hover.into(),
                                )),
                                border: Border {
                                    radius: theme.cosmic().radius_xl().into(),
                                    ..Default::default()
                                },
                                ..appearance(theme)
                            },
                            button::Status::Pressed | button::Status::Disabled => appearance(theme),
                        },
                    ))
                },
            )
            .into()
        });
        // TODO if there is a popup_index, create a button with a popup for the remaining workspaces
        // Should it appear on hover or on click?
        let layout_section: Element<_> = match self.layout {
            Layout::Row => row(buttons).spacing(4).into(),
            Layout::Column => column(buttons).spacing(4).into(),
        };
        let mut limits = Limits::NONE.min_width(1.).min_height(1.);
        if let Some(b) = self.core.applet.suggested_bounds {
            if b.width as i32 > 0 {
                limits = limits.max_width(b.width);
            }
            if b.height as i32 > 0 {
                limits = limits.max_height(b.height);
            }
        }

        autosize::autosize(
            container(layout_section).padding(0),
            AUTOSIZE_MAIN_ID.clone(),
        )
        .limits(limits)
        .into()
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            workspaces().map(Message::WorkspaceUpdate),
            event::listen_with(|e, _, _| match e {
                Mouse(mouse::Event::WheelScrolled { delta }) => Some(Message::WheelScrolled(delta)),
                _ => None,
            }),
        ])
    }

    fn style(&self) -> Option<cosmic::iced::theme::Style> {
        Some(cosmic::applet::style())
    }
}

/// Keeps the focused output's workspaces that have windows or are active, ordered by index.
/// Empty workspaces stay hidden until they become active, matching niri's dynamic workspaces.
fn visible_workspaces(all: Vec<Workspace>) -> Vec<Workspace> {
    let output = all
        .iter()
        .find(|w| w.is_focused)
        .or_else(|| all.iter().find(|w| w.is_active))
        .and_then(|w| w.output.clone());
    let mut list: Vec<Workspace> = all
        .into_iter()
        .filter(|w| w.output == output && (w.is_active || w.active_window_id.is_some()))
        .collect();
    list.sort_by_key(|w| w.idx);
    list
}

fn send_action(action: Action) {
    std::thread::spawn(move || {
        let result = Socket::connect()
            .and_then(|mut socket| socket.send(niri_ipc::Request::Action(action)))
            .and_then(|reply| reply.map_err(io::Error::other));
        if let Err(err) = result {
            tracing::error!("niri action failed: {err}");
        }
    });
}

fn roman(value: u8) -> String {
    const NUMERALS: [(u8, &str); 9] = [
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
        (0, ""),
        (0, ""),
    ];
    let mut remaining = value;
    let mut out = String::new();
    for (unit, symbol) in NUMERALS {
        while unit > 0 && remaining >= unit {
            out.push_str(symbol);
            remaining -= unit;
        }
    }
    out
}

fn active_numeral(theme: &cosmic::Theme) -> cosmic::iced::widget::text::Style {
    cosmic::iced::widget::text::Style {
        color: Some(theme.cosmic().accent_button.on.into()),
        ..Default::default()
    }
}
