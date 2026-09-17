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
        player: PlaybackWebView,
        commands: mpsc::Receiver<LocalPlayerCommand>,
    }
    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl gpui::IntoElement {
            div()
        }
    }

    pub fn run() {
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            for line in io::stdin().lock().lines().map_while(Result::ok) {
                if let Ok(command) = serde_json::from_str::<Command>(&line)
                    && sender.send(command.into()).is_err()
                {
                    break;
                }
            }
        });
        Application::new().run(move |cx: &mut App| {
            let bounds =
                Bounds::new(gpui::point(px(-10_000.0), px(-10_000.0)), size(px(1.0), px(1.0)));
            let _ = cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    show: false,
                    focus: false,
                    ..WindowOptions::default()
                },
                move |window, cx| {
                    let player = create_playback_webview(window, |message| {
                        let _ = writeln!(io::stdout(), "{message}");
                        let _ = io::stdout().flush();
                    })
                    .expect("WebView2 player host could not create its native child");
                    cx.new(|cx: &mut Context<Host>| {
                        cx.spawn(async move |host, cx| {
                            loop {
                                cx.background_executor().timer(Duration::from_millis(25)).await;
                                if !host
                                    .update(cx, |host, _| {
                                        for command in host.commands.try_iter() {
                                            let shutdown =
                                                matches!(command, LocalPlayerCommand::Shutdown);
                                            let _ = host.player.send(&command);
                                            if shutdown {
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
                        })
                        .detach();
                        Host { player, commands: receiver }
                    })
                },
            );
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
