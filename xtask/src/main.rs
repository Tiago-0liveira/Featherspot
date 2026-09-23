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
        (Some("dev"), Some("cli") | None) => dev(),
        (Some("build-dist"), None) => build_dist(),
        (Some("verify-dist"), Some(artifact)) => verify(Path::new(artifact)),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "usage: cargo xtask <dev [cli]|build-dist|verify-dist ARTIFACT>",
        )),
    }
}

fn dev() -> io::Result<()> {
    cargo(&["build", "-p", "lspotify-player-host", "-p", "lspotify-cli", "--bin", "lspotify"])?;
    let profile = PathBuf::from("target/debug");
    let stage = profile.join("lspotify-dev");
    let helpers = stage.join("helpers");
    fs::create_dir_all(&helpers)?;
    copy_binary(
        &profile.join(binary_name("lspotify-player-host")),
        &helpers.join(binary_name("lspotify-player-host")),
    )?;
    let executable = binary_name("lspotify");
    copy_binary(&profile.join(&executable), &stage.join(&executable))?;
    let status = Command::new(stage.join(&executable))
        .env("LSPOTIFY_PLAYER_HOST", helpers.join(binary_name("lspotify-player-host")))
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
        "lspotify-cli",
        "--bin",
        "lspotify",
        "-p",
        "lspotify-player-host",
    ])?;
    let release = PathBuf::from("target/release");
    let binary = |name| release.join(binary_name(name));
    #[cfg(target_os = "windows")]
    {
        let root = release.join("dist/lspotify");
        copy_binary(&binary("lspotify"), &root.join(binary_name("lspotify")))?;
        copy_binary(
            &binary("lspotify-player-host"),
            &root.join("helpers").join(binary_name("lspotify-player-host")),
        )?;
    }
    #[cfg(target_os = "macos")]
    {
        let root = release.join("dist/lspotify.app/Contents");
        copy_binary(&binary("lspotify"), &root.join("MacOS/lspotify"))?;
        copy_binary(&binary("lspotify-player-host"), &root.join("Helpers/lspotify-player-host"))?;
        copy_file(&PathBuf::from("packaging/macos/Info.plist"), &root.join("Info.plist"))?;
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let root = release.join("dist/lspotify.AppDir");
        copy_binary(&binary("lspotify"), &root.join("usr/bin/lspotify"))?;
        copy_binary(
            &binary("lspotify-player-host"),
            &root.join("usr/lib/lspotify/lspotify-player-host"),
        )?;
        copy_file(
            &PathBuf::from("packaging/linux/lspotify.desktop"),
            &root.join("usr/share/applications/lspotify.desktop"),
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
