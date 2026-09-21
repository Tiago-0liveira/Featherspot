#![forbid(unsafe_code)]

//! Private `WebView2` owner for `mellowdeck-cli`.
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
    use mellowdeck_core::LocalPlayerCommand;
    use mellowdeck_platform::{PlaybackWebView, create_playback_webview};
    use serde::Deserialize;

    #[derive(Debug, Deserialize)]
    #[serde(tag = "type", rename_all = "snake_case")]
    enum Command {
        TokenUpdate { access_token: String },
        Connect,
        Disconnect,
        Play,
        Pause,
        Seek { position_ms: u64 },
        Volume { value_milli: u16 },
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
                Command::Seek { position_ms } => Self::Seek { position_ms },
                Command::Volume { value_milli } => Self::Volume { value_milli },
                Command::Shutdown => Self::Shutdown,
            }
        }
    }

    struct Host {
        player: Option<PlaybackWebView>,
        commands: mpsc::Receiver<LocalPlayerCommand>,
        stdin_thread: Option<thread::JoinHandle<()>>,
    }
    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl gpui::IntoElement {
            div()
        }
    }

    impl Drop for Host {
        fn drop(&mut self) {
            drop(std::mem::replace(&mut self.commands, mpsc::sync_channel(1).1));
            if let Some(handle) = self.stdin_thread.take() {
                let _ = handle.join();
            }
        }
    }

    #[allow(clippy::too_many_lines)]
    pub fn run() {
        let (sender, receiver) = mpsc::sync_channel(64);
        let stdin_thread = thread::Builder::new()
            .name("mellowdeck-player-host-stdin".into())
            .spawn(move || {
                for line in io::stdin().lock().lines().map_while(Result::ok) {
                    if let Ok(command) = serde_json::from_str::<Command>(&line)
                        && sender.send(command.into()).is_err()
                    {
                        break;
                    }
                }
            })
            .ok();
        let mut receiver = Some(receiver);
        let mut stdin_thread = stdin_thread;
        Application::new().run(move |cx: &mut App| {
            let bounds =
                Bounds::new(gpui::point(px(-10_000.0), px(-10_000.0)), size(px(1.0), px(1.0)));
            let mut receiver = receiver.take();
            let mut stdin_thread = stdin_thread.take();
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
                                stdin_thread: stdin_thread.take(),
                            });
                        }
                    };
                    let host_receiver = receiver.take().expect("commands receiver");
                    let host_stdin_thread = stdin_thread.take();
                    cx.new(|cx: &mut Context<Host>| {
                        cx.spawn(async move |host, cx| {
                            loop {
                                cx.background_executor().timer(Duration::from_millis(25)).await;
                                if !host
                                    .update(cx, |host, cx| {
                                        for command in host.commands.try_iter() {
                                            let shutdown =
                                                matches!(command, LocalPlayerCommand::Shutdown);
                                            if let Some(player) = &host.player {
                                                let _ = player.send(&command);
                                            }
                                            if shutdown {
                                                cx.quit();
                                                return false;
                                            }
                                        }
                                        true
                                    })
                                    .unwrap_or(false)
                                {
                                    break;
                                }
                            }
                            let _ = host.update(cx, |host, cx| {
                                drop(std::mem::replace(&mut host.commands, mpsc::sync_channel(1).1));
                                if let Some(handle) = host.stdin_thread.take() {
                                    let _ = handle.join();
                                }
                                cx.quit();
                            });
                        })
                        .detach();
                        Host {
                            player: Some(player),
                            commands: host_receiver,
                            stdin_thread: host_stdin_thread,
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
    eprintln!("mellowdeck-player-host is supported on Windows only");
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
}

