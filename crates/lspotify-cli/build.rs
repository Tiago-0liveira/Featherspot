fn main() {
    println!("cargo:rerun-if-env-changed=LSPOTIFY_VERSION");
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/refs");

    if std::env::var("LSPOTIFY_VERSION").is_err()
        && let Ok(output) =
            std::process::Command::new("git").args(["describe", "--tags", "--abbrev=0"]).output()
        && output.status.success()
    {
        let tag = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let version = tag.strip_prefix('v').unwrap_or(&tag).to_string();
        if !version.is_empty() {
            println!("cargo:rustc-env=LSPOTIFY_VERSION={version}");
        }
    }
}
