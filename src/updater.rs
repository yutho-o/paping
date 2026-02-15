use colored::Colorize;
use serde::Deserialize;
use std::io::Read;

const REPO_OWNER: &str = "Yutho-tv";
const REPO_NAME: &str = "paping";
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    html_url: String,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

pub fn run_update() {
    println!("Checking for updates...");
    println!(
        "Current version: {}",
        format!("v{}", CURRENT_VERSION).green()
    );

    let url = format!(
        "https://api.github.com/repos/{}/{}/releases/latest",
        REPO_OWNER, REPO_NAME
    );

    let response = match ureq::get(&url).set("User-Agent", "paping-updater").call() {
        Ok(resp) => resp,
        Err(ureq::Error::Status(404, _)) => {
            println!("{}", "No releases found. You are on the latest version.".green());
            return;
        }
        Err(e) => {
            eprintln!("Error checking for updates: {}", e);
            return;
        }
    };

    let release: Release = match response.into_json() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Error parsing release info: {}", e);
            return;
        }
    };

    let latest_version = release.tag_name.trim_start_matches('v');

    if latest_version == CURRENT_VERSION {
        println!("{}", "You are already on the latest version!".green());
        return;
    }

    println!(
        "New version available: {}",
        format!("v{}", latest_version).green()
    );

    // Find the binary that matches our OS and architecture
    let target_name = get_target_asset_name();
    println!("Looking for a compatible binary for {}...", target_name);
    if let Some(asset) = release
        .assets
        .iter()
        .find(|a| a.name.to_lowercase().contains(&target_name))
    {
        println!("Downloading {}...", asset.name.green());
        match download_and_replace(&asset.browser_download_url) {
            Ok(_) => println!("{}", "Update successful! Restart paping to use the new version.".green()),
            Err(e) => {
                eprintln!("Auto-update failed: {}", e);
                println!(
                    "Please download manually from: {}",
                    release.html_url.cyan()
                );
            }
        }
    } else {
        println!(
            "No pre-built binary found for your platform ({}).",
            target_name
        );
        println!(
            "Please download manually from: {}",
            release.html_url.cyan()
        );
    }
}

fn get_target_asset_name() -> String {
    let os = if cfg!(target_os = "windows") {
        "win"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "unknown"
    };

    let arch = if cfg!(target_arch = "x86_64") {
        "amd64"
    } else if cfg!(target_arch = "aarch64") {
        "arm64"
    } else if cfg!(target_arch = "x86") {
        "i686"
    } else {
        "unknown"
    };

    format!("paping-{}-{}", os, arch)
}

fn download_and_replace(url: &str) -> Result<(), Box<dyn std::error::Error>> {
    let response = ureq::get(url)
        .set("User-Agent", "paping-updater")
        .call()?;

    let mut bytes = Vec::new();
    response.into_reader().read_to_end(&mut bytes)?;

    let current_exe = std::env::current_exe()?;
    let backup = current_exe.with_extension("old");

    // On Windows we can't overwrite a running .exe, so we rename
    // the current one to .old, then write the new binary
    if backup.exists() {
        std::fs::remove_file(&backup)?;
    }
    std::fs::rename(&current_exe, &backup)?;
    std::fs::write(&current_exe, &bytes)?;

    // On Linux/macOS, make the new binary executable
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&current_exe, std::fs::Permissions::from_mode(0o755))?;
    }

    // Remove the old backup file (no big deal if it fails)
    let _ = std::fs::remove_file(&backup);

    Ok(())
}
