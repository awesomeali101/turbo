
use anyhow::{anyhow, Context, Result};
use clap::{Arg, ArgAction, Command};
use dialoguer::{Confirm, MultiSelect};
use duct::cmd;
use indicatif::{ProgressBar, ProgressStyle};
use petgraph::graph::DiGraph;
use petgraph::algo::toposort;
use reqwest::blocking::Client;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command as PCommand, Stdio};
use std::time::Duration;
use std::thread;

mod config;
mod pac;
mod aur;
mod build;
mod ui;

use crate::aur::{AurInfo, AurMeta, AurRpcResponse};
use crate::config::Config;
use crate::ui::{pick_updates, pick_updates_numeric, Pickable};
use crate::build::{clone_aur_pkgs, regen_srcinfo, makepkg_build, collect_zsts, open_file_manager, ensure_persistent_dirs, clean_dir_contents, clean_cache};

fn main() -> Result<()> {
    let matches = Command::new("aurwrap")
        .about("A Rust AUR helper that wraps pacman: clones and builds AUR pkgs, installs them all at once with pacman -U")
        .arg(Arg::new("sync").short('S').action(ArgAction::SetTrue).help("Sync / install mode (pacman -S ...)"))
        .arg(Arg::new("refresh").short('y').action(ArgAction::Count).help("Refresh databases (can be doubled, like -yy)"))
        .arg(Arg::new("sysupgrade").short('u').action(ArgAction::SetTrue).help("System upgrade"))
        .arg(Arg::new("args").num_args(0..).trailing_var_arg(true).allow_hyphen_values(true).help("Additional pacman-like args or package names"))
        .get_matches();

    let cfg = Config::load()?;
    ensure_persistent_dirs(&cfg)?;

    let sync = matches.get_flag("sync");
    let ycount = matches.get_count("refresh");
    let sysupgrade = matches.get_flag("sysupgrade");
    let args: Vec<String> = matches
        .get_many::<String>("args")
        .map(|v| v.map(|s| s.to_string()).collect())
        .unwrap_or_else(Vec::new);

    // Special handling for -Scc: run pacman cache clean, then wipe our cache contents (keep dir)
    if args.iter().any(|a| a == "-Scc") {
        pac::sudo_pacman_scc()?;
        clean_cache(&cfg)?;
        return Ok(());
    }

    if sync && (sysupgrade || ycount > 0) && args.is_empty() {
        // Treat as -Syu or -Syyu: show update menu for AUR packages (Trizen-like).
        return handle_sysupgrade(&cfg, ycount as u8);
    }

    if sync {
        // Install specific packages: split between repo and AUR, build AUR in temp, install all together.
        return handle_sync(&cfg, &args);
    }

    // Pass-through to pacman for everything else.
    pac::passthrough_to_pacman(&args)?;
    Ok(())
}

fn handle_sysupgrade(cfg: &Config, ycount: u8) -> Result<()> {
    // If requested, refresh sync databases first (-y / -yy)
    if ycount > 0 {
        let mut flags = vec!["-Syu"]; 
        if ycount > 1 {
            flags = vec!["-Syyu"]; 
        }
        pac::run_pacman(&flags)?;
        thread::sleep(Duration::from_secs(1));
    }

    // Foreign packages (installed that are not in repos) - typically AUR ones.
    let foreign = pac::list_foreign_packages()?; // name -> version
    if foreign.is_empty() {
        println!("No foreign (AUR) packages installed.");
        return Ok(());
    }

    // Query AUR for latest versions
    let client = Client::builder().user_agent("aurwrap/0.1").build()?;
    let infos = aur::aur_info_batch(&client, foreign.keys().cloned().collect())?; // name -> AurInfo

    // Collect outdated (AUR version strictly newer than installed using pacman's vercmp)
    let mut outdated: Vec<Pickable> = vec![];
    for (name, curver) in foreign.iter() {
        if let Some(info) = infos.get(name) {
            if let Ok(ord) = pac::vercmp(curver, &info.version) {
                if ord < 0 { // installed < aur
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
        println!("All AUR packages are up to date.");
        return Ok(());
    }

    let selection = pick_updates_numeric(&outdated)?;
    if selection.is_empty() {
        println!("No packages selected.");
        return Ok(());
    }

    // Resolve dependencies and build order for selected updates
    let order = aur::resolve_build_order(&client, &selection)?;
    let temp_path = cfg.temp_dir();
    clean_dir_contents(&temp_path)?; // start with a clean temp each run

    // Track failures
    let mut clone_failed: Vec<String> = vec![];
    let mut build_failed: Vec<String> = vec![];
    let mut built_ok: Vec<String> = vec![];

    // Clone each, continue on error
    for name in &order {
        if let Err(e) = clone_aur_pkgs(cfg, &[name.clone()], &temp_path) {
            eprintln!("Clone failed for {}: {}", name, e);
            clone_failed.push(name.clone());
        }
    }

    // Offer edit
    let edit = Confirm::new().with_prompt("Edit PKGBUILDs/source files in file manager before building?").default(false).interact()?;
    if edit {
        open_file_manager(cfg, &temp_path)?;
        // After user returns, regenerate .SRCINFO for all
        for name in &order {
            regen_srcinfo(&temp_path.join(name))?;
        }
    }

    // Build
    for name in &order {
        if clone_failed.contains(name) { continue; }
        match makepkg_build(&temp_path.join(name)) {
            Ok(()) => built_ok.push(name.clone()),
            Err(e) => {
                eprintln!("Build failed for {}: {}", name, e);
                build_failed.push(name.clone());
            }
        }
    }

    // Gather artifacts and install with single pacman -U (with prompt)
    let zsts = collect_zsts(&temp_path)?;
    if zsts.is_empty() {
        return Err(anyhow!("No built *.pkg.tar.zst artifacts found."));
    }
    let mut install_failed: Vec<String> = vec![];
    let install_res = pac::sudo_pacman_U(&zsts);
    if install_res.is_err() {
        // Best-effort map artifacts to pkg names
        install_failed = built_ok.clone();
    }
    if let Err(e) = install_res { eprintln!("Install failed: {}", e); }
    
    // Summary
    if !clone_failed.is_empty() || !build_failed.is_empty() || !install_failed.is_empty() {
        println!("\nSummary:");
        if !clone_failed.is_empty() { println!("- Clone failed: {}", clone_failed.join(", ")); }
        if !build_failed.is_empty() { println!("- Build failed: {}", build_failed.join(", ")); }
        if !install_failed.is_empty() { println!("- Install failed: {}", install_failed.join(", ")); }
    }
    // Clean temp after completion
    clean_dir_contents(&temp_path)?;
    Ok(())
}

fn handle_sync(cfg: &Config, pkgs: &[String]) -> Result<()> {
    if pkgs.is_empty() {
        return Err(anyhow!("No packages specified. Did you mean -Syu?"));
    }
    // Determine which are repo vs AUR
    let (repo, aur) = pac::split_repo_vs_aur(pkgs)?;

    let client = Client::builder().user_agent("aurwrap/0.1").build()?;
    if !aur.is_empty() {
        // Determine AUR availability up-front to report unfound
        let info_map = aur::aur_info_batch(&client, aur.clone())?;
        let mut unfound: Vec<String> = aur.iter().filter(|n| !info_map.contains_key(*n)).cloned().collect();
        let available: Vec<String> = aur.iter().filter(|n| info_map.contains_key(*n)).cloned().collect();

        let build_order = aur::resolve_build_order(&client, &available)?;
        let temp_path = cfg.temp_dir();
        clean_dir_contents(&temp_path)?;
        // Track failures
        let mut clone_failed: Vec<String> = vec![];
        let mut build_failed: Vec<String> = vec![];
        let mut built_ok: Vec<String> = vec![];
        // Clone each, continue on error
        for name in &build_order {
            if let Err(e) = clone_aur_pkgs(cfg, &[name.clone()], &temp_path) {
                eprintln!("Clone failed for {}: {}", name, e);
                clone_failed.push(name.clone());
            }
        }

        // Prompt edit
        let edit = Confirm::new().with_prompt("Edit PKGBUILDs/source files in file manager before building?").default(false).interact()?;
        if edit {
            open_file_manager(cfg, &temp_path)?;
            for name in &build_order {
                regen_srcinfo(&temp_path.join(name))?;
            }
        }

        // Build each in order
        for name in &build_order {
            if clone_failed.contains(name) { continue; }
            match makepkg_build(&temp_path.join(name)) {
                Ok(()) => built_ok.push(name.clone()),
                Err(e) => { eprintln!("Build failed for {}: {}", name, e); build_failed.push(name.clone()); }
            }
        }

        // Collect .zst paths
        let zsts = collect_zsts(&temp_path)?;
        if zsts.is_empty() {
            return Err(anyhow!("No built *.pkg.tar.zst artifacts found."));
        }

        // Install everything together: repo names + built AUR files
        let mut install_failed: Vec<String> = vec![];
        let install_res = pac::sudo_pacman_U_with_repo(&repo, &zsts);
        if install_res.is_err() {
            install_failed = built_ok.clone();
        }
        if let Err(e) = install_res { eprintln!("Install failed: {}", e); }
        
        // Summary
        if !unfound.is_empty() || !clone_failed.is_empty() || !build_failed.is_empty() || !install_failed.is_empty() {
            println!("\nSummary:");
            if !unfound.is_empty() { println!("- Unfound in AUR: {}", unfound.join(", ")); }
            if !clone_failed.is_empty() { println!("- Clone failed: {}", clone_failed.join(", ")); }
            if !build_failed.is_empty() { println!("- Build failed: {}", build_failed.join(", ")); }
            if !install_failed.is_empty() { println!("- Install failed: {}", install_failed.join(", ")); }
        }
        // Clean temp after completion
        clean_dir_contents(&temp_path)?;
    } else {
        // Only repo packages
        if !repo.is_empty() {
            let mut args: Vec<&str> = vec!["-S"];
            for r in &repo { args.push(r.as_str()); }
            pac::run_pacman(&args)?;
        }
    }
    Ok(())
}
