use anyhow::{anyhow, Result};
use clap::{Arg, ArgAction, Command};
use dialoguer::Confirm;
use home::home_dir;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::time::Duration;
use tokio::time::sleep;

use crate::style::*;

mod aur;
mod build;
mod config;
mod pac;
mod self_update;
mod style;
mod ui;

use crate::build::{
    clean_cache, clean_dir_contents, clone_aur_pkgs, collect_zsts, ensure_persistent_dirs,
    makepkg_build, open_file_manager, regen_srcinfo, AurCloneSpec, AurSource,
};
use crate::build::{import_validpgpkeys, verify_sources};
use crate::config::Config;
use crate::self_update::ensure_latest_release_installed;
use crate::ui::{pick_updates_numeric, Pickable};

#[tokio::main]
async fn main() -> Result<()> {
    let matches = Command::new("aurwrap")
        .about("A Rust AUR helper that wraps pacman: clones and builds AUR pkgs, installs them all at once with pacman -U")
        .arg(Arg::new("sync").short('S').action(ArgAction::SetTrue).help("Sync / install mode (pacman -S ...)"))
        .arg(Arg::new("refresh").short('y').action(ArgAction::Count).help("Refresh databases (can be doubled, like -yy)"))
        .arg(Arg::new("sysupgrade").short('u').action(ArgAction::SetTrue).help("System upgrade"))
        .arg(Arg::new("print_updates").short('P').action(ArgAction::SetTrue).help("Print list of packages that need to be upgraded"))
        .arg(Arg::new("noconfirm").long("noconfirm").action(ArgAction::SetTrue).help("No confirm mode (pacman -U --noconfirm)"))
        .arg(Arg::new("args").num_args(0..).trailing_var_arg(true).allow_hyphen_values(true).help("Additional pacman-like args or package names"))
        .get_matches();

    let cfg = Config::load()?;
    ensure_persistent_dirs(&cfg)?;

    let sync = matches.get_flag("sync");
    let ycount = matches.get_count("refresh");
    let sysupgrade = matches.get_flag("sysupgrade");
    let print_updates = matches.get_flag("print_updates");
    let args: Vec<String> = matches
        .get_many::<String>("args")
        .map(|v| v.map(|s| s.to_string()).collect())
        .unwrap_or_else(Vec::new);

    // Handle -P: print list of packages that need to be upgraded
    // Check both the flag and args in case it wasn't parsed as a flag
    if print_updates || args.iter().any(|a| a == "-P") {
        let forcerefresh = ycount > 1;

        return handle_print_updates(&cfg, forcerefresh).await;
    }

    // Special handling for -Scc: run pacman cache clean, then wipe our cache contents (keep dir)
    if args.iter().any(|a| a == "-Scc") {
        pac::sudo_pacman_scc()?;
        clean_cache(&cfg)?;
        return Ok(());
    }

    if sync && (sysupgrade || ycount > 0) && args.is_empty() {
        // Treat as -Syu or -Syyu: show update menu for AUR packages (Trizen-like).
        return handle_sysupgrade(&cfg, ycount as u8, &matches).await;
    }

    if sync {
        // Install specific packages: split between repo and AUR, build AUR in temp, install all together.
        return handle_sync(&cfg, &args, &matches);
    }

    // Pass-through to pacman for everything else.
    let _ = pac::passthrough_to_pacman(&args).await?;
    Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
struct PackageUpdate {
    name: String,
    old_version: String,
    new_version: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct UpdateList {
    aur: Vec<PackageUpdate>,
    pacman: Vec<PackageUpdate>,
}

#[derive(Clone, Debug)]
struct AurRequest {
    name: String,
    display: String,
    source: AurSource,
}

fn split_repo_notation(arg: &str) -> Option<(&str, &str)> {
    if arg.starts_with('-') {
        return None;
    }
    let mut iter = arg.splitn(2, '/');
    let repo = iter.next()?;
    let pkg = iter.next()?;
    if repo.is_empty() || pkg.is_empty() {
        return None;
    }
    Some((repo, pkg))
}

fn classify_sync_targets(cfg: &Config, pkgs: &[String]) -> Result<(Vec<String>, Vec<AurRequest>)> {
    let default_source = AurSource::from_cfg(cfg);
    let mut repo_pkgs: Vec<String> = vec![];
    let mut aur_pkgs: Vec<AurRequest> = vec![];
    let mut needs_detection: Vec<String> = vec![];

    for pkg in pkgs {
        if pkg.starts_with('-') {
            repo_pkgs.push(pkg.clone());
            continue;
        }
        if let Some((repo, name)) = split_repo_notation(pkg) {
            match repo {
                _ if repo.eq_ignore_ascii_case("aur") => aur_pkgs.push(AurRequest {
                    name: name.to_string(),
                    display: pkg.clone(),
                    source: AurSource::Official,
                }),
                _ if repo.eq_ignore_ascii_case("github-aur") => aur_pkgs.push(AurRequest {
                    name: name.to_string(),
                    display: pkg.clone(),
                    source: AurSource::Github,
                }),
                _ => repo_pkgs.push(pkg.clone()),
            }
        } else {
            needs_detection.push(pkg.clone());
        }
    }

    if !needs_detection.is_empty() {
        let (repo_detected, aur_detected) = pac::split_repo_vs_aur(&needs_detection)?;
        let mut repo_counts: HashMap<String, usize> = HashMap::new();
        for name in repo_detected {
            *repo_counts.entry(name).or_insert(0) += 1;
        }
        let mut aur_counts: HashMap<String, usize> = HashMap::new();
        for name in aur_detected {
            *aur_counts.entry(name).or_insert(0) += 1;
        }

        for name in needs_detection {
            if let Some(count) = repo_counts.get_mut(&name) {
                if *count > 0 {
                    repo_pkgs.push(name.clone());
                    *count -= 1;
                    continue;
                }
            }
            if let Some(count) = aur_counts.get_mut(&name) {
                if *count > 0 {
                    aur_pkgs.push(AurRequest {
                        display: name.clone(),
                        name,
                        source: default_source,
                    });
                    *count -= 1;
                    continue;
                }
            }
        }
    }

    Ok((repo_pkgs, aur_pkgs))
}

async fn handle_print_updates(_cfg: &Config, forcerefresh: bool) -> Result<()> {
    let client = Client::builder().user_agent("aurwrap/0.1").build()?;

    // Get outdated AUR packages
    let foreign = pac::list_foreign_packages().await?;
    let mut aur_updates = Vec::<PackageUpdate>::new();

    if !foreign.is_empty() {
        let infos = aur::aur_info_batch(&client, foreign.keys().cloned().collect())?;
        for (name, curver) in foreign.iter() {
            if let Some(info) = infos.get(name) {
                if let Ok(ord) = pac::vercmp(curver, &info.version).await {
                    if ord < 0 {
                        // installed < aur
                        aur_updates.push(PackageUpdate {
                            name: name.clone(),
                            old_version: curver.clone(),
                            new_version: info.version.clone(),
                        });
                    }
                }
            }
        }
    }

    // Get outdated pacman packages
    let pacman_outdated = pac::list_outdated_pacman_packages(forcerefresh).await?;
    let pacman_updates: Vec<PackageUpdate> = pacman_outdated
        .into_iter()
        .map(|(name, old_ver, new_ver)| PackageUpdate {
            name,
            old_version: old_ver,
            new_version: new_ver,
        })
        .collect();

    // Display AUR updates
    println!(
        "\n{} {}",
        section_title().apply_to("AUR Packages to Update"),
        aur_badge()
    );
    if aur_updates.is_empty() {
        println!(
            "  {} {}",
            info_icon(),
            dim().apply_to("No AUR packages need updating.")
        );
    } else {
        for pkg in &aur_updates {
            let name = package_name().apply_to(&pkg.name);
            let old_ver = current_version().apply_to(&pkg.old_version);
            let arrow = dim().apply_to("→");
            let new_ver = new_version().apply_to(&pkg.new_version);
            println!(
                "  {} {name:<32} {old_ver:>12}  {arrow}  {new_ver:<12}",
                bullet(),
                name = name,
                old_ver = old_ver,
                arrow = arrow,
                new_ver = new_ver
            );
        }
    }

    // Display pacman updates
    println!(
        "\n{} {}",
        section_title().apply_to("Repo Packages to Update"),
        pacman_badge()
    );
    if pacman_updates.is_empty() {
        println!(
            "  {} {}",
            info_icon(),
            dim().apply_to("No repo packages need updating.")
        );
    } else {
        for pkg in &pacman_updates {
            let name = package_name().apply_to(&pkg.name);
            let old_ver = current_version().apply_to(&pkg.old_version);
            let arrow = dim().apply_to("→");
            let new_ver = new_version().apply_to(&pkg.new_version);
            println!(
                "  {} {name:<32} {old_ver:>12}  {arrow}  {new_ver:<12}",
                bullet(),
                name = name,
                old_ver = old_ver,
                arrow = arrow,
                new_ver = new_ver
            );
        }
    }

    // Write JSON file
    let update_list = UpdateList {
        aur: aur_updates,
        pacman: pacman_updates,
    };

    let json_path = home_dir()
        .ok_or_else(|| anyhow!("Cannot determine home directory"))?
        .join("turbo")
        .join("needupdate.json");

    // Ensure directory exists
    if let Some(parent) = json_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let json_content = serde_json::to_string_pretty(&update_list)?;
    fs::write(&json_path, json_content)?;

    println!(
        "\n{} {} {}",
        info_icon(),
        highlight().apply_to("JSON output written to"),
        path().apply_to(json_path.display())
    );

    Ok(())
}

async fn handle_sysupgrade(cfg: &Config, ycount: u8, arg_matches: &clap::ArgMatches) -> Result<()> {
    // If requested, refresh sync databases first (-y / -yy)
    if ycount > 0 {
        let mut flags = vec![String::from("-Syu")];
        if ycount > 1 {
            flags = vec![String::from("-Syyu")];
        }
        let command_str = format!("Running: sudo pacman {}", flags[0].as_str());
        println!(
            "{} {} {}",
            info_icon(),
            pacman_badge(),
            prompt().apply_to(command_str.as_str())
        );
        pac::run_pacman(&flags).await?;
        sleep(Duration::from_secs(3)).await;
    }

    if ycount > 1 {
        ensure_latest_release_installed(cfg)?;
    }

    // Foreign packages (installed that are not in repos) - typically AUR ones.
    let foreign = pac::list_foreign_packages().await?; // name -> version
    if foreign.is_empty() {
        println!(
            "{} {}",
            info_icon(),
            dim().apply_to("No foreign (AUR) packages installed.")
        );
        return Ok(());
    }

    // Query AUR for latest versions
    let client = Client::builder().user_agent("aurwrap/0.1").build()?;
    let infos = aur::aur_info_batch(&client, foreign.keys().cloned().collect())?; // name -> AurInfo

    // Collect outdated (AUR version strictly newer than installed using pacman's vercmp)
    let mut outdated: Vec<Pickable> = vec![];
    for (name, curver) in foreign.iter() {
        if let Some(info) = infos.get(name) {
            if let Ok(ord) = pac::vercmp(curver, &info.version).await {
                if ord < 0 {
                    // installed < aur
                    outdated.push(Pickable {
                        name: name.clone(),
                        current: curver.clone(),
                        latest: info.version.clone(),
                    });
                }
            }
        }
    }

    if outdated.is_empty() {
        println!(
            "{} {}",
            success_icon(),
            success().apply_to("All AUR packages are up to date.")
        );
        return Ok(());
    }

    let selection = pick_updates_numeric(&outdated)?;
    if selection.is_empty() {
        println!(
            "{} {}",
            info_icon(),
            dim().apply_to("No packages selected.")
        );
        return Ok(());
    }

    // Resolve dependencies and build order for selected updates (by package names)
    let order = aur::resolve_build_order(&client, &selection)?;
    let temp_path = cfg.temp_dir();
    clean_dir_contents(&temp_path)?; // start with a clean temp each run

    // Track failures
    let mut clone_failed: Vec<String> = vec![]; // track by pkgbase
    let mut build_failed: Vec<String> = vec![]; // track by pkgbase
    let mut built_ok: Vec<String> = vec![]; // track by pkgbase

    // Group targets by AUR pkgbase: only clone/build unique pkgbase repos
    let info_for_order = aur::aur_info_batch(&client, order.clone())?; // name -> AurInfo
    let mut seen_base: HashSet<String> = HashSet::new();
    let mut pkgbases: Vec<String> = vec![];
    for name in &order {
        if let Some(info) = info_for_order.get(name) {
            if seen_base.insert(info.pkgbase.clone()) {
                pkgbases.push(info.pkgbase.clone());
            }
        }
    }

    // Clone each, continue on error
    let default_source = AurSource::from_cfg(cfg);
    for base in &pkgbases {
        let spec = AurCloneSpec {
            pkgbase: base.clone(),
            source: default_source,
        };
        if let Err(e) = clone_aur_pkgs(cfg, std::slice::from_ref(&spec), &temp_path) {
            let pretty_base = format!("{}", package_name().apply_to(base));
            eprintln!(
                "{} {} {}",
                error_icon(),
                aur_badge(),
                error().apply_to(format!("Clone failed for {}: {}", pretty_base, e))
            );
            clone_failed.push(base.clone());
        }
    }

    // Offer edit
    let edit = Confirm::new()
        .with_prompt("Edit PKGBUILDs/source files in file manager before building?")
        .default(false)
        .interact()?;
    if edit {
        open_file_manager(cfg, &temp_path)?;
        // After user returns, regenerate .SRCINFO for all
        for base in &pkgbases {
            regen_srcinfo(&temp_path.join(base))?;
        }
    }

    // Verify sources (and import keys) then build
    for base in &pkgbases {
        if clone_failed.contains(base) {
            continue;
        }
        let dir = temp_path.join(base);
        // Try to import valid PGP keys (best effort)
        let _ = import_validpgpkeys(&dir);
        // Verify sources before committing to a long build
        if let Err(e) = verify_sources(&dir) {
            let pretty_base = format!("{}", package_name().apply_to(base));
            eprintln!(
                "{} {} {}",
                warn_icon(),
                aur_badge(),
                warning().apply_to(format!(
                    "Source verification failed for {}: {}",
                    pretty_base, e
                ))
            );
            build_failed.push(base.clone());
            continue;
        }
        match makepkg_build(&dir) {
            Ok(()) => built_ok.push(base.clone()),
            Err(e) => {
                let pretty_base = format!("{}", package_name().apply_to(base));
                eprintln!(
                    "{} {} {}",
                    error_icon(),
                    aur_badge(),
                    error().apply_to(format!("Build failed for {}: {}", pretty_base, e))
                );
                build_failed.push(base.clone());
            }
        }
    }

    // Gather artifacts and install with single pacman -U (with or without prompt)
    let zsts = collect_zsts(&temp_path)?;
    if zsts.is_empty() {
        return Err(anyhow!("No built *.pkg.tar.zst artifacts found."));
    }
    let mut install_failed: Vec<String> = vec![];
    let install_res = if arg_matches.get_flag("noconfirm") {
        pac::sudo_pacman_U_noconfirm(&zsts)
    } else {
        pac::sudo_pacman_U(&zsts)
    };
    if install_res.is_err() {
        install_failed = built_ok.clone();
    }
    if let Err(e) = install_res {
        eprintln!(
            "{} {} {}",
            error_icon(),
            pacman_badge(),
            error().apply_to(format!("Install failed: {}", e))
        );
    }

    // Summary
    if !clone_failed.is_empty() || !build_failed.is_empty() || !install_failed.is_empty() {
        println!("\n{} {}", section_title().apply_to("Summary"), aur_badge());
        if !clone_failed.is_empty() {
            println!(
                "  {} {}",
                warn_icon(),
                highlight().apply_to(format!("Clone failed: {}", clone_failed.join(", ")))
            );
        }
        if !build_failed.is_empty() {
            println!(
                "  {} {}",
                warn_icon(),
                highlight().apply_to(format!("Build failed: {}", build_failed.join(", ")))
            );
        }
        if !install_failed.is_empty() {
            println!(
                "  {} {}",
                error_icon(),
                highlight_value()
                    .apply_to(format!("Install failed: {}", install_failed.join(", ")))
            );
        }
    }
    // Clean temp after completion
    clean_dir_contents(&temp_path)?;
    Ok(())
}

fn handle_sync(cfg: &Config, pkgs: &[String], arg_matches: &clap::ArgMatches) -> Result<()> {
    if pkgs.is_empty() {
        return Err(anyhow!("No packages specified. Did you mean -Syu?"));
    }
    // Determine which are repo vs AUR (with optional repo prefixes)
    let (repo, aur_requests) = classify_sync_targets(cfg, pkgs)?;
    let repo_noconfirm = arg_matches.get_flag("noconfirm");
    if !repo.is_empty() {
        pac::install_repo_packages(&repo, repo_noconfirm)?;
    }

    if aur_requests.is_empty() {
        return Ok(());
    }

    let client = Client::builder().user_agent("aurwrap/0.1").build()?;
    let requested_names: Vec<String> = aur_requests.iter().map(|req| req.name.clone()).collect();
    // Determine AUR availability up-front to report unfound
    let info_map = aur::aur_info_batch(&client, requested_names)?;
    let unfound: Vec<String> = aur_requests
        .iter()
        .filter(|req| !info_map.contains_key(&req.name))
        .map(|req| req.display.clone())
        .collect();
    let available: Vec<String> = aur_requests
        .iter()
        .filter(|req| info_map.contains_key(&req.name))
        .map(|req| req.name.clone())
        .collect();

    let build_order = aur::resolve_build_order(&client, &available)?;
    let temp_path = cfg.temp_dir();
    clean_dir_contents(&temp_path)?;
    // Track failures by pkgbase
    let mut clone_failed: Vec<String> = vec![];
    let mut build_failed: Vec<String> = vec![];
    let mut built_ok: Vec<String> = vec![];

    // Group by pkgbase: only clone unique bases
    let info_for_order = aur::aur_info_batch(&client, build_order.clone())?; // name -> AurInfo
    let mut seen_base: HashSet<String> = HashSet::new();
    let mut pkgbases: Vec<String> = vec![];
    for name in &build_order {
        if let Some(info) = info_for_order.get(name) {
            if seen_base.insert(info.pkgbase.clone()) {
                pkgbases.push(info.pkgbase.clone());
            }
        }
    }
    let mut pkgbase_sources: HashMap<String, AurSource> = HashMap::new();
    for req in &aur_requests {
        if let Some(info) = info_for_order.get(&req.name) {
            pkgbase_sources
                .entry(info.pkgbase.clone())
                .or_insert(req.source);
        }
    }

    // Clone each base, continue on error
    let default_source = AurSource::from_cfg(cfg);
    for base in &pkgbases {
        let source = pkgbase_sources.get(base).copied().unwrap_or(default_source);
        let spec = AurCloneSpec {
            pkgbase: base.clone(),
            source,
        };
        if let Err(e) = clone_aur_pkgs(cfg, std::slice::from_ref(&spec), &temp_path) {
            let badge = match source {
                AurSource::Github => github_aur_mirror_badge(),
                AurSource::Official => aur_badge(),
            };
            let pretty_base = format!("{}", package_name().apply_to(base));
            eprintln!(
                "{} {} {}",
                error_icon(),
                badge,
                error().apply_to(format!("Clone failed for {}: {}", pretty_base, e))
            );
            clone_failed.push(base.clone());
        }
    }

    // Prompt edit
    let edit = Confirm::new()
        .with_prompt("Edit PKGBUILDs/source files in file manager before building?")
        .default(false)
        .interact()?;
    if edit {
        open_file_manager(cfg, &temp_path)?;
        for base in &pkgbases {
            regen_srcinfo(&temp_path.join(base))?;
        }
    }

    // Verify sources then build each in order
    for base in &pkgbases {
        if clone_failed.contains(base) {
            continue;
        }
        let dir = temp_path.join(base);
        let _ = import_validpgpkeys(&dir);
        if let Err(e) = verify_sources(&dir) {
            let source = pkgbase_sources.get(base).copied().unwrap_or(default_source);
            let badge = match source {
                AurSource::Github => github_aur_mirror_badge(),
                AurSource::Official => aur_badge(),
            };
            let pretty_base = format!("{}", package_name().apply_to(base));
            eprintln!(
                "{} {} {}",
                warn_icon(),
                badge,
                warning().apply_to(format!(
                    "Source verification failed for {}: {}",
                    pretty_base, e
                ))
            );
            build_failed.push(base.clone());
            continue;
        }
        match makepkg_build(&dir) {
            Ok(()) => built_ok.push(base.clone()),
            Err(e) => {
                let source = pkgbase_sources.get(base).copied().unwrap_or(default_source);
                let badge = match source {
                    AurSource::Github => github_aur_mirror_badge(),
                    AurSource::Official => aur_badge(),
                };
                let pretty_base = format!("{}", package_name().apply_to(base));
                eprintln!(
                    "{} {} {}",
                    error_icon(),
                    badge,
                    error().apply_to(format!("Build failed for {}: {}", pretty_base, e))
                );
                build_failed.push(base.clone());
            }
        }
    }

    // Collect .zst paths
    let zsts = collect_zsts(&temp_path)?;
    if zsts.is_empty() {
        return Err(anyhow!("No built *.pkg.tar.zst artifacts found."));
    }

    // Install built AUR files
    let mut install_failed: Vec<String> = vec![];
    let install_res = if repo_noconfirm {
        pac::sudo_pacman_U_noconfirm(&zsts)
    } else {
        pac::sudo_pacman_U(&zsts)
    };
    if install_res.is_err() {
        install_failed = built_ok.clone();
    }
    if let Err(e) = install_res {
        eprintln!(
            "{} {} {}",
            error_icon(),
            pacman_badge(),
            error().apply_to(format!("Install failed: {}", e))
        );
    }

    // Summary
    if !unfound.is_empty()
        || !clone_failed.is_empty()
        || !build_failed.is_empty()
        || !install_failed.is_empty()
    {
        println!("\n{} {}", section_title().apply_to("Summary"), aur_badge());
        if !unfound.is_empty() {
            println!(
                "  {} {}",
                warn_icon(),
                highlight().apply_to(format!("Unfound: {}", unfound.join(", ")))
            );
        }
        if !clone_failed.is_empty() {
            println!(
                "  {} {}",
                warn_icon(),
                highlight().apply_to(format!("Clone failed: {}", clone_failed.join(", ")))
            );
        }
        if !build_failed.is_empty() {
            println!(
                "  {} {}",
                warn_icon(),
                highlight().apply_to(format!("Build failed: {}", build_failed.join(", ")))
            );
        }
        if !install_failed.is_empty() {
            println!(
                "  {} {}",
                error_icon(),
                highlight_value()
                    .apply_to(format!("Install failed: {}", install_failed.join(", ")))
            );
        }
    }
    // Clean temp after completion
    clean_dir_contents(&temp_path)?;
    Ok(())
}
