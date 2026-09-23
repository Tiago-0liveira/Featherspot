#![forbid(unsafe_code)]

use std::{
    env, fs, io,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("xtask: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> io::Result<()> {
    let mut arguments = env::args().skip(1);
    match (arguments.next().as_deref(), arguments.next().as_deref()) {
        (Some("dev"), Some(frontend @ ("gui" | "cli"))) => dev(frontend),
        (Some("build-dist"), None) => build_dist(),
        (Some("verify-dist"), Some(artifact)) => verify(Path::new(artifact)),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "usage: cargo xtask <dev gui|dev cli|build-dist|verify-dist ARTIFACT>",
        )),
    }
}

fn dev(frontend: &str) -> io::Result<()> {
    cargo(&[
        "build",
        "-p",
        "mellowdeck-player-host",
        "-p",
        if frontend == "gui" { "mellowdeck" } else { "mellowdeck-cli" },
    ])?;
    let profile = PathBuf::from("target/debug");
    let stage = profile.join("mellowdeck-dev");
    let helpers = stage.join("helpers");
    fs::create_dir_all(&helpers)?;
    copy_binary(
        &profile.join(binary_name("mellowdeck-player-host")),
        &helpers.join(binary_name("mellowdeck-player-host")),
    )?;
    let executable = binary_name(if frontend == "gui" { "mellowdeck" } else { "mellowdeck-cli" });
    copy_binary(&profile.join(&executable), &stage.join(&executable))?;
    let status = Command::new(stage.join(&executable))
        .env("MELLOWDECK_PLAYER_HOST", helpers.join(binary_name("mellowdeck-player-host")))
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other("development frontend exited unsuccessfully"))
    }
}

fn build_dist() -> io::Result<()> {
    cargo(&[
        "build",
        "--release",
        "-p",
        "mellowdeck",
        "-p",
        "mellowdeck-cli",
        "-p",
        "mellowdeck-player-host",
    ])?;
    let release = PathBuf::from("target/release");
    let binary = |name| release.join(binary_name(name));
    #[cfg(target_os = "windows")]
    {
        let root = release.join("dist/Mellowdeck");
        copy_binary(&binary("mellowdeck"), &root.join(binary_name("mellowdeck")))?;
        copy_binary(
            &binary("mellowdeck-player-host"),
            &root.join("helpers").join(binary_name("mellowdeck-player-host")),
        )?;
    }
    #[cfg(target_os = "macos")]
    {
        let root = release.join("dist/Mellowdeck.app/Contents");
        copy_binary(&binary("mellowdeck"), &root.join("MacOS/mellowdeck"))?;
        copy_binary(
            &binary("mellowdeck-player-host"),
            &root.join("Helpers/mellowdeck-player-host"),
        )?;
        copy_file(&PathBuf::from("packaging/macos/Info.plist"), &root.join("Info.plist"))?;
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let root = release.join("dist/Mellowdeck.AppDir");
        copy_binary(&binary("mellowdeck"), &root.join("usr/bin/mellowdeck"))?;
        copy_binary(
            &binary("mellowdeck-player-host"),
            &root.join("usr/lib/mellowdeck/mellowdeck-player-host"),
        )?;
        copy_file(
            &PathBuf::from("packaging/linux/mellowdeck.desktop"),
            &root.join("usr/share/applications/mellowdeck.desktop"),
        )?;
    }
    Ok(())
}
fn verify(artifact: &Path) -> io::Result<()> {
    if artifact.is_file() {
        Ok(())
    } else {
        Err(io::Error::new(io::ErrorKind::NotFound, "distribution artifact was not found"))
    }
}
fn cargo(arguments: &[&str]) -> io::Result<()> {
    let status = Command::new("cargo").args(arguments).status()?;
    if status.success() { Ok(()) } else { Err(io::Error::other("cargo build failed")) }
}
fn copy_binary(source: &PathBuf, destination: &PathBuf) -> io::Result<u64> {
    copy_file(source, destination)
}
fn copy_file(source: &PathBuf, destination: &PathBuf) -> io::Result<u64> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(source, destination)
}
fn binary_name(name: &str) -> String {
    if cfg!(windows) { format!("{name}.exe") } else { name.to_owned() }
}
