
use anyhow::{anyhow, Result};
use clap::{Arg, ArgAction, Command};
use dialoguer::Confirm;
use reqwest::blocking::Client;
use std::collections::HashSet;
use std::thread;
use std::time::Duration;
use serde::{Serialize, Deserialize};
use std::fs;
use home::home_dir;

use crate::style::*;

mod config;
mod pac;
mod aur;
mod build;
mod ui;
mod style;

use crate::config::Config;
use crate::build::{import_validpgpkeys, verify_sources};
use crate::ui::{pick_updates_numeric, Pickable};
use crate::build::{clone_aur_pkgs, regen_srcinfo, makepkg_build, collect_zsts, open_file_manager, ensure_persistent_dirs, clean_dir_contents, clean_cache};

fn main() -> Result<()> {
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
    if print_updates {
        return handle_print_updates(&cfg);
    }

    // Special handling for -Scc: run pacman cache clean, then wipe our cache contents (keep dir)
    if args.iter().any(|a| a == "-Scc") {
        pac::sudo_pacman_scc()?;
        clean_cache(&cfg)?;
        return Ok(());
    }

    if sync && (sysupgrade || ycount > 0) && args.is_empty() {
        // Treat as -Syu or -Syyu: show update menu for AUR packages (Trizen-like).
        return handle_sysupgrade(&cfg, ycount as u8, &matches);
    }

    if sync {
        // Install specific packages: split between repo and AUR, build AUR in temp, install all together.
        return handle_sync(&cfg, &args, &matches);
    }

    // Pass-through to pacman for everything else.
    pac::passthrough_to_pacman(&args)?;
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

fn handle_print_updates(cfg: &Config) -> Result<()> {
    let client = Client::builder().user_agent("aurwrap/0.1").build()?;
    
    // Get outdated AUR packages
    let foreign = pac::list_foreign_packages()?;
    let mut aur_updates = Vec::<PackageUpdate>::new();
    
    if !foreign.is_empty() {
        let infos = aur::aur_info_batch(&client, foreign.keys().cloned().collect())?;
        for (name, curver) in foreign.iter() {
            if let Some(info) = infos.get(name) {
                if let Ok(ord) = pac::vercmp(curver, &info.version) {
                    if ord < 0 { // installed < aur
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
    let pacman_outdated = pac::list_outdated_pacman_packages()?;
    let pacman_updates: Vec<PackageUpdate> = pacman_outdated
        .into_iter()
        .map(|(name, old_ver, new_ver)| PackageUpdate {
            name,
            old_version: old_ver,
            new_version: new_ver,
        })
        .collect();
    
    // Display AUR updates
    println!("\n{} AUR Packages to Update:", header().apply_to("==>"));
    if aur_updates.is_empty() {
        println!("  {}", dim().apply_to("No AUR packages need updating."));
    } else {
        for pkg in &aur_updates {
            let name = package_name().apply_to(&pkg.name);
            let old_ver = current_version().apply_to(&pkg.old_version);
            let arrow = dim().apply_to("->");
            let new_ver = new_version().apply_to(&pkg.new_version);
            println!("  {name:<32} {old_ver:>12}  {arrow}  {new_ver:<12}", 
                name = name, old_ver = old_ver, arrow = arrow, new_ver = new_ver);
        }
    }
    
    // Display pacman updates
    println!("\n{} Pacman Packages to Update:", header().apply_to("==>"));
    if pacman_updates.is_empty() {
        println!("  {}", dim().apply_to("No pacman packages need updating."));
    } else {
        for pkg in &pacman_updates {
            let name = package_name().apply_to(&pkg.name);
            let old_ver = current_version().apply_to(&pkg.old_version);
            let arrow = dim().apply_to("->");
            let new_ver = new_version().apply_to(&pkg.new_version);
            println!("  {name:<32} {old_ver:>12}  {arrow}  {new_ver:<12}", 
                name = name, old_ver = old_ver, arrow = arrow, new_ver = new_ver);
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
    
    println!("\n{} JSON output written to: {}", 
        header().apply_to("==>"),
        path().apply_to(json_path.display()));
    
    Ok(())
}

fn handle_sysupgrade(cfg: &Config, ycount: u8, arg_matches: &clap::ArgMatches) -> Result<()> {
    // If requested, refresh sync databases first (-y / -yy)
    if ycount > 0 {
        let mut flags = vec!["-Syu"]; 
        if ycount > 1 {
            flags = vec!["-Syyu"]; 
        }
        println!(
            "{} {}",
            header().apply_to("==>"),
            prompt().apply_to("Synchronizing package databases...")
        );
        pac::run_pacman(&flags)?;
        thread::sleep(Duration::from_secs(1));
    }

    // Foreign packages (installed that are not in repos) - typically AUR ones.
    let foreign = pac::list_foreign_packages()?; // name -> version
    if foreign.is_empty() {
        println!(
            "{} {}",
            header().apply_to("==>"),
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

    // Resolve dependencies and build order for selected updates (by package names)
    let order = aur::resolve_build_order(&client, &selection)?;
    let temp_path = cfg.temp_dir();
    clean_dir_contents(&temp_path)?; // start with a clean temp each run

    // Track failures
    let mut clone_failed: Vec<String> = vec![];  // track by pkgbase
    let mut build_failed: Vec<String> = vec![];  // track by pkgbase
    let mut built_ok: Vec<String> = vec![];      // track by pkgbase

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
    for base in &pkgbases {
        if let Err(e) = clone_aur_pkgs(cfg, &[base.clone()], &temp_path) {
            eprintln!("Clone failed for {}: {}", base, e);
            clone_failed.push(base.clone());
        }
    }

    // Offer edit
    let edit = Confirm::new().with_prompt("Edit PKGBUILDs/source files in file manager before building?").default(false).interact()?;
    if edit {
        open_file_manager(cfg, &temp_path)?;
        // After user returns, regenerate .SRCINFO for all
        for base in &pkgbases {
            regen_srcinfo(&temp_path.join(base))?;
        }
    }

    // Verify sources (and import keys) then build
    for base in &pkgbases {
        if clone_failed.contains(base) { continue; }
        let dir = temp_path.join(base);
        // Try to import valid PGP keys (best effort)
        let _ = import_validpgpkeys(&dir);
        // Verify sources before committing to a long build
        if let Err(e) = verify_sources(&dir) {
            eprintln!("Source verification failed for {}: {}", base, e);
            build_failed.push(base.clone());
            continue;
        }
        match makepkg_build(&dir) {
            Ok(()) => built_ok.push(base.clone()),
            Err(e) => {
                eprintln!("Build failed for {}: {}", base, e);
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

fn handle_sync(cfg: &Config, pkgs: &[String], arg_matches: &clap::ArgMatches) -> Result<()> {
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

        // Clone each base, continue on error
        for base in &pkgbases {
            if let Err(e) = clone_aur_pkgs(cfg, &[base.clone()], &temp_path) {
                eprintln!("Clone failed for {}: {}", base, e);
                clone_failed.push(base.clone());
            }
        }

        // Prompt edit
        let edit = Confirm::new().with_prompt("Edit PKGBUILDs/source files in file manager before building?").default(false).interact()?;
        if edit {
            open_file_manager(cfg, &temp_path)?;
            for base in &pkgbases {
                regen_srcinfo(&temp_path.join(base))?;
            }
        }

        // Verify sources then build each in order
        for base in &pkgbases {
            if clone_failed.contains(base) { continue; }
            let dir = temp_path.join(base);
            let _ = import_validpgpkeys(&dir);
            if let Err(e) = verify_sources(&dir) {
                eprintln!("Source verification failed for {}: {}", base, e);
                build_failed.push(base.clone());
                continue;
            }
            match makepkg_build(&dir) {
                Ok(()) => built_ok.push(base.clone()),
                Err(e) => { eprintln!("Build failed for {}: {}", base, e); build_failed.push(base.clone()); }
            }
        }

        // Collect .zst paths
        let zsts = collect_zsts(&temp_path)?;
        if zsts.is_empty() {
            return Err(anyhow!("No built *.pkg.tar.zst artifacts found."));
        }

        // Install everything together: repo names + built AUR files
        let mut install_failed: Vec<String> = vec![];
        let install_res = if arg_matches.get_flag("noconfirm") {
            pac::sudo_pacman_U_with_repo_noconfirm(&repo, &zsts)
        } else {
            pac::sudo_pacman_U_with_repo(&repo, &zsts)
        };
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
