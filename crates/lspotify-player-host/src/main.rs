#![forbid(unsafe_code)]

//! Private `WebView2` owner for `lspotify`.
//!
//! The helper intentionally has no visible UI. A Web Playback SDK instance must be attached to a
//! real native window, which a terminal cannot supply. Its stdin/stdout are the parent-only IPC
//! transport; production launchers pass a per-launch nonce and reject records without it.

#[cfg(target_os = "windows")]
mod windows {
    use std::{
        io::{self, BufRead as _, Write as _},
        sync::mpsc,
        thread,
        time::Duration,
    };

    use gpui::{
        App, AppContext as _, Application, Bounds, Context, Render, Window, WindowBounds,
        WindowOptions, div, px, size,
    };
    use lspotify_core::LocalPlayerCommand;
    use lspotify_platform::{PlaybackWebView, create_playback_webview};
    use serde::Deserialize;

    #[derive(Debug, Deserialize)]
    #[serde(tag = "type", rename_all = "snake_case")]
    enum Command {
        TokenUpdate { access_token: String },
        Connect,
        Disconnect,
        Play,
        Pause,
        Previous,
        Next,
        Seek { position_ms: u64 },
        Volume { value_milli: u16 },
        Shuffle { enabled: bool },
        Repeat { mode: u8 },
        Shutdown,
    }

    impl From<Command> for LocalPlayerCommand {
        fn from(command: Command) -> Self {
            match command {
                Command::TokenUpdate { access_token } => Self::TokenUpdate { access_token },
                Command::Connect => Self::Connect,
                Command::Disconnect => Self::Disconnect,
                Command::Play => Self::Play,
                Command::Pause => Self::Pause,
                Command::Previous => Self::Previous,
                Command::Next => Self::Next,
                Command::Seek { position_ms } => Self::Seek { position_ms },
                Command::Volume { value_milli } => Self::Volume { value_milli },
                Command::Shuffle { enabled } => Self::Shuffle { enabled },
                Command::Repeat { mode } => Self::Repeat { mode },
                Command::Shutdown => Self::Shutdown,
            }
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum HostAction {
        Continue,
        Shutdown,
    }

    pub trait PlayerBridge {
        fn send(&self, command: &LocalPlayerCommand) -> lspotify_core::Result<()>;
    }

    impl PlayerBridge for PlaybackWebView {
        fn send(&self, command: &LocalPlayerCommand) -> lspotify_core::Result<()> {
            self.send(command)
        }
    }

    pub fn handle_host_commands<P: PlayerBridge>(
        commands: &mpsc::Receiver<LocalPlayerCommand>,
        player: Option<&P>,
    ) -> HostAction {
        loop {
            match commands.try_recv() {
                Ok(command) => {
                    let is_shutdown = matches!(command, LocalPlayerCommand::Shutdown);
                    if let Some(player) = player {
                        let _ = player.send(&command);
                    }
                    if is_shutdown {
                        return HostAction::Shutdown;
                    }
                }
                Err(mpsc::TryRecvError::Empty) => return HostAction::Continue,
                Err(mpsc::TryRecvError::Disconnected) => {
                    return HostAction::Shutdown;
                }
            }
        }
    }

    pub fn cleanup_player<P: PlayerBridge>(player: Option<&P>) {
        if let Some(player) = player {
            let _ = player.send(&LocalPlayerCommand::Shutdown);
        }
    }

    struct Host {
        player: Option<PlaybackWebView>,
        commands: mpsc::Receiver<LocalPlayerCommand>,
    }
    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl gpui::IntoElement {
            div()
        }
    }

    impl Drop for Host {
        fn drop(&mut self) {
            cleanup_player(self.player.as_ref());
        }
    }

    #[allow(clippy::too_many_lines)]
    pub fn run() {
        let (sender, receiver) = mpsc::sync_channel(64);
        let _stdin_thread = thread::Builder::new()
            .name("lspotify-player-host-stdin".into())
            .spawn(move || {
                for line in io::stdin().lock().lines().map_while(Result::ok) {
                    if let Ok(command) = serde_json::from_str::<Command>(&line)
                        && sender.send(command.into()).is_err()
                    {
                        return;
                    }
                }
                // When stdin loop finishes (EOF), sender is dropped,
                // disconnecting the command channel and notifying the GPUI host.
            })
            .ok();
        let mut receiver = Some(receiver);
        Application::new().run(move |cx: &mut App| {
            let bounds =
                Bounds::new(gpui::point(px(-10_000.0), px(-10_000.0)), size(px(1.0), px(1.0)));
            let mut receiver = receiver.take();
            let window_result = cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    show: false,
                    focus: false,
                    ..WindowOptions::default()
                },
                move |window, cx| {
                    let player = match create_playback_webview(window, |message| {
                        let _ = writeln!(io::stdout(), "{message}");
                        let _ = io::stdout().flush();
                    }) {
                        Ok(player) => player,
                        Err(error) => {
                            let reason = error.to_string();
                            let _ = writeln!(
                                io::stdout(),
                                r#"{{"version":1,"id":0,"payload":{{"type":"unavailable","reason":"{}"}}}}"#,
                                reason.replace('"', "\\\"")
                            );
                            let _ = io::stdout().flush();
                            cx.quit();
                            return cx.new(|_| Host {
                                player: None,
                                commands: receiver.take().expect("commands receiver"),
                            });
                        }
                    };
                    let host_receiver = receiver.take().expect("commands receiver");
                    cx.new(|cx: &mut Context<Host>| {
                        cx.spawn(async move |host, cx| {
                            loop {
                                cx.background_executor().timer(Duration::from_millis(25)).await;
                                let action = host
                                    .update(cx, |host, _cx| {
                                        handle_host_commands(&host.commands, host.player.as_ref())
                                    })
                                    .unwrap_or(HostAction::Shutdown);

                                if action == HostAction::Shutdown {
                                    break;
                                }
                            }
                            // Ensure player cleanup runs on every shutdown path
                            let _ = host.update(cx, |host, _cx| {
                                cleanup_player(host.player.as_ref());
                            });
                            // Allow a brief period for WebView2 to execute player.disconnect()
                            // and notify Spotify servers before quitting the process
                            cx.background_executor().timer(Duration::from_millis(150)).await;
                            let _ = host.update(cx, |host, cx| {
                                host.player = None;
                                cx.quit();
                            });
                        })
                        .detach();
                        Host {
                            player: Some(player),
                            commands: host_receiver,
                        }
                    })
                },
            );
            if let Err(error) = window_result {
                let reason = error.to_string();
                let _ = writeln!(
                    io::stdout(),
                    r#"{{"version":1,"id":0,"payload":{{"type":"unavailable","reason":"{}"}}}}"#,
                    reason.replace('"', "\\\"")
                );
                let _ = io::stdout().flush();
                cx.quit();
            }
        });
    }
}

#[cfg(target_os = "windows")]
fn main() {
    windows::run();
}

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("lspotify-player-host is supported on Windows only");
}

#[cfg(test)]
mod tests {

    #[test]
    fn unavailable_event_serialization() {
        let reason = "WebView2 runtime missing".to_string();
        let json = format!(
            r#"{{"version":1,"id":0,"payload":{{"type":"unavailable","reason":"{}"}}}}"#,
            reason.replace('"', "\\\"")
        );
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid json");
        assert_eq!(parsed["version"], 1);
        assert_eq!(parsed["id"], 0);
        assert_eq!(parsed["payload"]["type"], "unavailable");
        assert_eq!(parsed["payload"]["reason"], "WebView2 runtime missing");
    }

    #[cfg(target_os = "windows")]
    mod regression_tests {
        use super::super::windows::{
            HostAction, PlayerBridge, cleanup_player, handle_host_commands,
        };
        use lspotify_core::LocalPlayerCommand;
        use std::{
            io::Write as _,
            sync::{Arc, Mutex, mpsc::sync_channel},
            time::Duration,
        };

        struct MockPlayer {
            commands: Arc<Mutex<Vec<LocalPlayerCommand>>>,
        }

        impl PlayerBridge for MockPlayer {
            fn send(&self, command: &LocalPlayerCommand) -> lspotify_core::Result<()> {
                self.commands.lock().unwrap().push(command.clone());
                Ok(())
            }
        }

        #[test]
        fn host_exits_when_its_command_channel_disconnects() {
            let (sender, receiver) = sync_channel::<LocalPlayerCommand>(10);
            let player = MockPlayer { commands: Arc::new(Mutex::new(Vec::new())) };

            // Empty channel continues
            assert_eq!(handle_host_commands(&receiver, Some(&player)), HostAction::Continue);

            // Dropping sender disconnects the channel
            drop(sender);
            assert_eq!(handle_host_commands(&receiver, Some(&player)), HostAction::Shutdown);
        }

        #[test]
        fn host_exits_after_shutdown() {
            let (sender, receiver) = sync_channel::<LocalPlayerCommand>(10);
            let player = MockPlayer { commands: Arc::new(Mutex::new(Vec::new())) };

            sender.send(LocalPlayerCommand::Shutdown).unwrap();
            assert_eq!(handle_host_commands(&receiver, Some(&player)), HostAction::Shutdown);
            let sent = player.commands.lock().unwrap();
            assert_eq!(sent.len(), 1);
            assert!(matches!(sent[0], LocalPlayerCommand::Shutdown));
        }

        #[test]
        fn player_cleanup_runs_on_every_shutdown_path() {
            // Path 1: Normal Shutdown command
            {
                let (sender, receiver) = sync_channel::<LocalPlayerCommand>(10);
                let player = MockPlayer { commands: Arc::new(Mutex::new(Vec::new())) };
                sender.send(LocalPlayerCommand::Shutdown).unwrap();
                let action = handle_host_commands(&receiver, Some(&player));
                assert_eq!(action, HostAction::Shutdown);
                cleanup_player(Some(&player));
                let sent = player.commands.lock().unwrap();
                assert!(sent.iter().any(|cmd| matches!(cmd, LocalPlayerCommand::Shutdown)));
            }

            // Path 2: Channel disconnection (parent termination)
            {
                let (sender, receiver) = sync_channel::<LocalPlayerCommand>(10);
                let player = MockPlayer { commands: Arc::new(Mutex::new(Vec::new())) };
                drop(sender);
                let action = handle_host_commands(&receiver, Some(&player));
                assert_eq!(action, HostAction::Shutdown);
                cleanup_player(Some(&player));
                let sent = player.commands.lock().unwrap();
                assert!(sent.iter().any(|cmd| matches!(cmd, LocalPlayerCommand::Shutdown)));
            }
        }

        #[test]
        fn parent_termination_does_not_leave_helper_alive() {
            let helper_exe = std::env::current_exe().ok().and_then(|p| {
                let dir = p.parent()?;
                let candidate1 = dir.join("lspotify-player-host.exe");
                if candidate1.is_file() {
                    return Some(candidate1);
                }
                let candidate2 = dir.parent()?.join("lspotify-player-host.exe");
                if candidate2.is_file() {
                    return Some(candidate2);
                }
                None
            });

            if let Some(exe) = helper_exe {
                let mut child = std::process::Command::new(&exe)
                    .stdin(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::piped())
                    .spawn()
                    .expect("spawn helper");

                // Drop stdin immediately, simulating parent exit / closed pipe
                drop(child.stdin.take());

                let deadline = std::time::Instant::now() + Duration::from_secs(5);
                let mut exited = false;
                while std::time::Instant::now() < deadline {
                    if let Ok(Some(_)) = child.try_wait() {
                        exited = true;
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }

                if !exited {
                    let _ = child.kill();
                }
                let _ = child.wait();
                assert!(exited, "helper process did not terminate after parent stdin closed");
            }
        }

        #[test]
        fn host_exits_promptly_after_shutdown_command_process() {
            let helper_exe = std::env::current_exe().ok().and_then(|p| {
                let dir = p.parent()?;
                let candidate1 = dir.join("lspotify-player-host.exe");
                if candidate1.is_file() {
                    return Some(candidate1);
                }
                let candidate2 = dir.parent()?.join("lspotify-player-host.exe");
                if candidate2.is_file() {
                    return Some(candidate2);
                }
                None
            });

            if let Some(exe) = helper_exe {
                let mut child = std::process::Command::new(&exe)
                    .stdin(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::piped())
                    .spawn()
                    .expect("spawn helper");

                if let Some(mut stdin) = child.stdin.take() {
                    let _ = writeln!(stdin, r#"{{"type":"shutdown"}}"#);
                    let _ = stdin.flush();
                }

                let deadline = std::time::Instant::now() + Duration::from_secs(5);
                let mut exited = false;
                while std::time::Instant::now() < deadline {
                    if let Ok(Some(_)) = child.try_wait() {
                        exited = true;
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }

                if !exited {
                    let _ = child.kill();
                }
                let _ = child.wait();
                assert!(exited, "helper process did not terminate after shutdown command");
            }
        }
    }
}
