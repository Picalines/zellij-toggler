use serde::{Deserialize, Serialize};
use serde_constant::ConstBool;
use std::{
    collections::{BTreeMap, HashMap},
    mem,
    path::PathBuf,
};
use zellij_tile::prelude::*;

#[derive(Clone)]
enum TogglerPaneState {
    /// Pane requested, waiting for CommandPaneOpened
    Opening { pipe_id: String, is_toggle: bool },
    /// Pane is open
    Opened { zellij_pane_id: u32 },
    /// Close requested, waiting for PaneClosed/CommandPaneExited
    Closing {
        zellij_pane_id: u32,
        pipe_id: String,
        is_toggle: bool,
    },
}

#[derive(Default)]
struct TogglerState {
    panes: HashMap<String, TogglerPaneState>,
}

register_plugin!(TogglerState);

#[derive(Clone, Deserialize)]
struct CommandConfig {
    cmd: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    cwd: Option<String>,
}

#[derive(Deserialize)]
struct OpenRequest {
    pane_id: String,
    #[serde(flatten)]
    command: CommandConfig,
}

#[derive(Deserialize)]
struct CloseRequest {
    pane_id: String,
}

#[derive(Deserialize)]
struct ToggleRequest {
    pane_id: String,
    #[serde(flatten)]
    command: CommandConfig,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum ToggleResponseAction {
    Opened,
    Closed,
}

#[derive(Serialize)]
struct ToggleResponse {
    ok: ConstBool<true>,
    action: ToggleResponseAction,
}

#[derive(Serialize)]
struct OkResponse {
    ok: ConstBool<true>,
}

#[derive(Serialize)]
struct WarningResponse {
    ok: ConstBool<true>,
    warning: String,
}

#[derive(Serialize)]
struct ErrorResponse {
    ok: ConstBool<false>,
    error: String,
}

fn cli_pipe_json_output<T: Serialize>(pipe_id: &str, body: &T) {
    let body_str = serde_json::to_string(body).unwrap_or_default();
    cli_pipe_output(pipe_id, &body_str);
    unblock_cli_pipe_input(pipe_id);
}

impl ZellijPlugin for TogglerState {
    fn load(&mut self, _configuration: BTreeMap<String, String>) {
        request_permission(&[
            PermissionType::RunCommands,
            PermissionType::ChangeApplicationState,
            PermissionType::ReadApplicationState,
            PermissionType::ReadCliPipes,
        ]);
        subscribe(&[
            EventType::CommandPaneOpened,
            EventType::CommandPaneExited,
            EventType::PaneClosed,
            EventType::PermissionRequestResult,
        ]);
    }

    fn update(&mut self, event: Event) -> bool {
        match event {
            Event::PermissionRequestResult(PermissionStatus::Granted) => {
                hide_self();
            }
            Event::CommandPaneOpened(pane_id, context) => {
                self.handle_pane_opened_event(pane_id, context);
            }
            Event::CommandPaneExited(pane_id, _exit_code, _context) => {
                self.handle_pane_exited_event(pane_id);
            }
            Event::PaneClosed(PaneId::Terminal(pane_id)) => {
                self.handle_pane_exited_event(pane_id);
            }
            _ => {}
        }
        false
    }

    fn pipe(&mut self, pipe_message: PipeMessage) -> bool {
        let PipeSource::Cli(pipe_id) = pipe_message.source else {
            return false;
        };

        let pipe_name = pipe_message.name.as_str();
        let payload = pipe_message.payload.as_deref().unwrap_or("");

        match pipe_name {
            "toggler::open" => {
                if let Some(req) = Self::payload_or_send_error::<OpenRequest>(&pipe_id, payload) {
                    self.handle_open_pipe(&pipe_id, &req);
                }
            }
            "toggler::close" => {
                if let Some(req) = Self::payload_or_send_error::<CloseRequest>(&pipe_id, payload) {
                    self.handle_close_pipe(&pipe_id, &req);
                }
            }
            "toggler::toggle" => {
                if let Some(req) = Self::payload_or_send_error::<ToggleRequest>(&pipe_id, payload) {
                    self.handle_toggle_pipe(&pipe_id, &req);
                }
            }
            _ => {
                cli_pipe_json_output(
                    &pipe_id,
                    &ErrorResponse {
                        ok: ConstBool,
                        error: format!("unknown command: {}", pipe_name),
                    },
                );
            }
        }

        false
    }

    fn render(&mut self, _rows: usize, _cols: usize) {
        // Hidden plugin - no rendering
    }
}

impl TogglerState {
    const PANE_ID_CONTEXT: &str = "__toggler_pane_id";

    fn payload_or_send_error<'d, T: Deserialize<'d>>(pipe_id: &str, payload: &'d str) -> Option<T> {
        match serde_json::from_str::<T>(payload) {
            Err(json_error) => {
                cli_pipe_json_output(
                    pipe_id,
                    &ErrorResponse {
                        ok: ConstBool,
                        error: format!("invalid json: {}", json_error),
                    },
                );
                None
            }
            Ok(parsed_payload) => Some(parsed_payload),
        }
    }

    fn handle_open_pipe(&mut self, pipe_id: &str, payload: &OpenRequest) {
        match self.panes.get(&payload.pane_id) {
            Some(TogglerPaneState::Opened { .. }) => {
                cli_pipe_json_output(
                    pipe_id,
                    &WarningResponse {
                        ok: ConstBool,
                        warning: "pane is already opened".to_string(),
                    },
                );
            }
            Some(TogglerPaneState::Opening { .. }) => {
                cli_pipe_json_output(
                    pipe_id,
                    &WarningResponse {
                        ok: ConstBool,
                        warning: "pane is already opening".to_string(),
                    },
                );
            }
            Some(TogglerPaneState::Closing { .. }) => {
                cli_pipe_json_output(
                    pipe_id,
                    &ErrorResponse {
                        ok: ConstBool,
                        error: "pane is closing".to_string(),
                    },
                );
            }
            None => {
                self.start_opening_pane(pipe_id, &payload.pane_id, false, &payload.command);
            }
        }
    }

    fn handle_close_pipe(&mut self, pipe_id: &str, payload: &CloseRequest) {
        match self.panes.get(&payload.pane_id) {
            Some(TogglerPaneState::Opened { zellij_pane_id }) => {
                self.start_closing_pane(pipe_id, &payload.pane_id, *zellij_pane_id, false);
            }
            Some(TogglerPaneState::Opening { .. }) => {
                cli_pipe_json_output(
                    pipe_id,
                    &ErrorResponse {
                        ok: ConstBool,
                        error: "pane is opening".to_string(),
                    },
                );
            }
            Some(TogglerPaneState::Closing { .. }) => {
                cli_pipe_json_output(
                    pipe_id,
                    &WarningResponse {
                        ok: ConstBool,
                        warning: "pane is already closing".to_string(),
                    },
                );
            }
            None => {
                cli_pipe_json_output(
                    pipe_id,
                    &WarningResponse {
                        ok: ConstBool,
                        warning: "pane not found".to_string(),
                    },
                );
            }
        }
    }

    fn handle_toggle_pipe(&mut self, pipe_id: &str, payload: &ToggleRequest) {
        match self.panes.get(&payload.pane_id) {
            Some(TogglerPaneState::Opened { zellij_pane_id }) => {
                self.start_closing_pane(pipe_id, &payload.pane_id, *zellij_pane_id, true);
            }
            Some(TogglerPaneState::Opening { .. }) | Some(TogglerPaneState::Closing { .. }) => {
                cli_pipe_json_output(
                    pipe_id,
                    &WarningResponse {
                        ok: ConstBool,
                        warning: "pane is transitioning".to_string(),
                    },
                );
            }
            None => {
                self.start_opening_pane(pipe_id, &payload.pane_id, true, &payload.command);
            }
        }
    }

    fn handle_pane_opened_event(&mut self, zellij_pane_id: u32, context: BTreeMap<String, String>) {
        let Some(pane_id) = context.get(Self::PANE_ID_CONTEXT) else {
            return;
        };

        let Some(pane_state) = self.panes.get_mut(pane_id) else {
            return;
        };

        let TogglerPaneState::Opening { pipe_id, is_toggle } =
            mem::replace(pane_state, TogglerPaneState::Opened { zellij_pane_id })
        else {
            return;
        };

        if is_toggle {
            cli_pipe_json_output(
                &pipe_id,
                &ToggleResponse {
                    ok: ConstBool,
                    action: ToggleResponseAction::Opened,
                },
            );
        } else {
            cli_pipe_json_output(&pipe_id, &OkResponse { ok: ConstBool });
        }
    }

    fn handle_pane_exited_event(&mut self, zellij_pane_id: u32) {
        let Some(pane_id) = self.find_pane_id_by_zellij_id(zellij_pane_id) else {
            return;
        };

        let Some(state) = self.panes.remove(&pane_id.clone()) else {
            return;
        };

        let TogglerPaneState::Closing {
            pipe_id, is_toggle, ..
        } = state
        else {
            return;
        };

        if is_toggle {
            cli_pipe_json_output(
                &pipe_id,
                &ToggleResponse {
                    ok: ConstBool,
                    action: ToggleResponseAction::Closed,
                },
            );
        } else {
            cli_pipe_json_output(&pipe_id, &OkResponse { ok: ConstBool });
        }
    }

    fn find_pane_id_by_zellij_id(&self, zellij_pane_id: u32) -> Option<&String> {
        self.panes
            .iter()
            .find(|(_, state)| match state {
                TogglerPaneState::Opened {
                    zellij_pane_id: id, ..
                }
                | TogglerPaneState::Closing {
                    zellij_pane_id: id, ..
                } => *id == zellij_pane_id,
                _ => false,
            })
            .map(|(pane_id, _)| pane_id)
    }

    fn start_opening_pane(
        &mut self,
        pipe_id: &str,
        pane_id: &str,
        is_toggle: bool,
        config: &CommandConfig,
    ) {
        block_cli_pipe_input(pipe_id);

        self.panes.insert(
            pane_id.to_string(),
            TogglerPaneState::Opening {
                pipe_id: pipe_id.to_string(),
                is_toggle,
            },
        );

        let mut cmd_context = BTreeMap::new();
        cmd_context.insert(Self::PANE_ID_CONTEXT.to_string(), pane_id.to_string());

        let mut cmd = CommandToRun::new_with_args(&config.cmd, config.args.clone());
        cmd.cwd = config.cwd.as_ref().map(PathBuf::from);
        open_command_pane(cmd, cmd_context);
    }

    fn start_closing_pane(
        &mut self,
        pipe_id: &str,
        pane_id: &str,
        zellij_pane_id: u32,
        is_toggle: bool,
    ) {
        block_cli_pipe_input(pipe_id);

        self.panes.insert(
            pane_id.to_string(),
            TogglerPaneState::Closing {
                zellij_pane_id,
                pipe_id: pipe_id.to_string(),
                is_toggle,
            },
        );
        close_terminal_pane(zellij_pane_id);
    }
}
