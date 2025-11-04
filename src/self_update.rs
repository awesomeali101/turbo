use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use duct::cmd;
use reqwest::blocking::Client;
use semver::Version;
use serde::Deserialize;

use crate::build::{clean_dir_contents, collect_zsts};
use crate::config::Config;
use crate::pac;
use crate::style::*;

const REPO_URL: &str = "https://github.com/splizer101/turbo.git";
const RELEASES_API: &str = "https://api.github.com/repos/splizer101/turbo/releases/latest";

#[derive(Debug, Deserialize)]
struct ReleaseResponse {
    tag_name: String,
    draft: bool,
    prerelease: bool,
}

pub fn ensure_latest_release_installed(cfg: &Config) -> Result<()> {
    let client = Client::builder()
        .user_agent("turbo-self-update/0.1")
        .timeout(Duration::from_secs(20))
        .build()?;

    let release = match fetch_latest_release(&client) {
        Ok(r) => r,
        Err(err) => {
            eprintln!(
                "{} {}",
                warn_icon(),
                warning().apply_to(format!("Unable to check latest Turbo release: {}", err))
            );
            return Ok(());
        }
    };

    let tag = release.tag_name.trim();
    let latest_version = normalize_tag(tag);
    let current_version = env!("CARGO_PKG_VERSION");

    let latest_semver =
        Version::parse(&latest_version).context("Parsing latest release version")?;
    let current_semver =
        Version::parse(current_version).context("Parsing current Turbo version")?;

    if latest_semver <= current_semver {
        return Ok(());
    }

    println!(
        "{} {} {} {} {}",
        info_icon(),
        highlight().apply_to("Turbo update available"),
        highlight_value().apply_to(current_version),
        dim().apply_to("→"),
        highlight_value().apply_to(&latest_version)
    );

    install_release(cfg, tag)?;
    Ok(())
}

fn fetch_latest_release(client: &Client) -> Result<ReleaseResponse> {
    let resp = client
        .get(RELEASES_API)
        .send()
        .context("GitHub release request failed")?
        .error_for_status()
        .context("GitHub release API returned an error status")?;
    let release: ReleaseResponse = resp.json().context("Invalid GitHub release payload")?;
    if release.draft {
        return Err(anyhow!("Latest tagged release is still a draft"));
    }
    if release.prerelease {
        return Err(anyhow!("Latest tagged release is marked as prerelease"));
    }
    Ok(release)
}

fn install_release(cfg: &Config, tag: &str) -> Result<()> {
    let temp_root = cfg.temp_dir().join("self-update");
    clean_dir_contents(&temp_root)?;
    fs::create_dir_all(&temp_root)?;

    let checkout_dir = temp_root.join("turbo");
    println!(
        "{} {} {}",
        info_icon(),
        highlight().apply_to("Fetching"),
        github_badge()
    );
    run_git_clone(tag, &checkout_dir)?;

    println!(
        "{} {} {}",
        info_icon(),
        highlight().apply_to("Building new Turbo release"),
        aur_badge()
    );
    run_makepkg(&checkout_dir)?;

    let artifacts = collect_zsts(&checkout_dir)?;
    if artifacts.is_empty() {
        return Err(anyhow!(
            "Self-update build produced no *.pkg.tar.zst artifacts"
        ));
    }

    println!(
        "{} {}",
        info_icon(),
        prompt().apply_to("Installing refreshed Turbo package...")
    );
    if cfg.noconfirm {
        pac::sudo_pacman_U_noconfirm(&artifacts)?;
    } else {
        pac::sudo_pacman_U(&artifacts)?;
    }
    println!(
        "{} {}",
        success_icon(),
        success().apply_to("Turbo updated successfully.")
    );
    Ok(())
}

fn run_git_clone(tag: &str, checkout_dir: &PathBuf) -> Result<()> {
    let status = cmd!(
        "git",
        "clone",
        "--depth",
        "1",
        "--branch",
        tag,
        REPO_URL,
        checkout_dir
    )
    .stderr_to_stdout()
    .run()
    .context("git clone failed")?;

    if !status.status.success() {
        return Err(anyhow!(
            "git clone exited with status {}",
            status.status.code().unwrap_or(-1)
        ));
    }
    Ok(())
}

fn run_makepkg(checkout_dir: &PathBuf) -> Result<()> {
    let build_cmd = format!("cd {} && makepkg -s -f --noconfirm", checkout_dir.display());
    let status = cmd!("bash", "-lc", build_cmd)
        .stderr_to_stdout()
        .run()
        .context("makepkg failed")?;
    if !status.status.success() {
        return Err(anyhow!(
            "makepkg exited with status {}",
            status.status.code().unwrap_or(-1)
        ));
    }
    Ok(())
}

fn normalize_tag(tag: &str) -> String {
    tag.trim_start_matches('v').to_string()
}
