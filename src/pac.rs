use crate::config::Config;
use crate::style::*;
use anyhow::{anyhow, Result};
use duct::cmd;
use std::collections::HashMap;
use std::sync::{LazyLock, OnceLock};
use tokio::task;

static PACMAN: OnceLock<String> = OnceLock::new();

pub fn get_pacman() -> &'static str {
    PACMAN.get_or_init(|| Config::load().unwrap().pacman)
}

pub async fn run_pacman(args: &[String]) -> Result<()> {
    let pacman = get_pacman();
    let mut full_args = vec![pacman.to_string()];
    full_args.extend(args.iter().cloned());
    let status =
        task::spawn_blocking(move || cmd("sudo", full_args).stderr_to_stdout().unchecked().run())
            .await??;
    if !status.status.success() {
        let exit_desc = status
            .status
            .code()
            .map(|code| code.to_string())
            .unwrap_or_else(|| String::from("terminated by signal"));
        let message = format!("{} {:?} exited with {}", pacman, args, exit_desc);
        println!(
            "{} {} {}",
            warn_icon(),
            pacman_badge(),
            warning().apply_to(message)
        );
    }
    Ok(())
}

pub fn is_in_repo(name: &str) -> Result<bool> {
    let pacman = get_pacman();
    let res = cmd(
        "bash",
        [
            "-lc",
            &format!("sudo {} -Si -- {}", pacman, shell_escape(name)),
        ],
    )
    .stdout_capture()
    .stderr_null()
    .unchecked()
    .run()?;
    let ok = res.status.success() && !String::from_utf8_lossy(&res.stdout).is_empty();
    Ok(ok)
}

pub async fn passthrough_to_pacman(args: &[String]) -> Result<bool> {
    let pacman = get_pacman();
    if args.is_empty() {
        return Ok(false);
    }
    let argstr = args.join(" ");
    println!(
        "{} {} {}",
        info_icon(),
        pacman_badge(),
        prompt().apply_to(format!("Running: sudo {} {}", pacman, argstr).as_str())
    );
    let owned = args.to_vec();
    run_pacman(&owned).await?;
    Ok(true)
}

pub async fn list_foreign_packages() -> Result<HashMap<String, String>> {
    // pacman -Qm : foreign; we'll get name and version
    let pacman = get_pacman();
    let out = task::spawn_blocking(move || cmd("sudo", [pacman, "-Qm"]).stderr_to_stdout().read())
        .await??;
    let mut map = HashMap::new();
    for line in out.lines() {
        if let Some((n, v)) = line.split_once(' ') {
            map.insert(n.to_string(), v.to_string());
        }
    }
    Ok(map)
}

pub async fn vercmp(a: &str, b: &str) -> Result<i32> {
    // pacman's vercmp prints -1, 0, or 1 on stdout
    let a = a.to_string();
    let b = b.to_string();
    let out = task::spawn_blocking(move || {
        cmd("vercmp", [a.as_str(), b.as_str()])
            .stderr_to_stdout()
            .read()
    })
    .await??;
    let trimmed = out.trim();
    let v: i32 = trimmed
        .parse()
        .map_err(|_| anyhow!("invalid vercmp output: {}", trimmed))?;
    Ok(v)
}

pub fn split_repo_vs_aur(pkgs: &[String]) -> Result<(Vec<String>, Vec<String>)> {
    let pacman = get_pacman();
    let mut repo = vec![];
    let mut aur = vec![];
    for p in pkgs {
        // If pacman -Si finds it in a repo, treat as repo; else assume AUR
        let res = cmd(
            "bash",
            [
                "-lc",
                &format!("sudo {} -Si -- {}", pacman, shell_escape(p)),
            ],
        )
        .stdout_capture()
        .stderr_null()
        .unchecked()
        .run()?;
        let ok = res.status.success() && !String::from_utf8_lossy(&res.stdout).is_empty();
        if ok {
            repo.push(p.clone());
        } else {
            aur.push(p.clone());
        }
    }
    Ok((repo, aur))
}

fn shell_escape(s: &str) -> String {
    let mut out = String::from("'");
    out.push_str(&s.replace('\'', "'\\''"));
    out.push('\'');
    out
}

pub fn sudo_pacman_U(zsts: &[String]) -> Result<()> {
    sudo_pacman_U_inner(zsts, false)
}

pub fn sudo_pacman_U_noconfirm(zsts: &[String]) -> Result<()> {
    sudo_pacman_U_inner(zsts, true)
}

fn sudo_pacman_U_inner(zsts: &[String], noconfirm: bool) -> Result<()> {
    let mut args = vec!["-U"];
    if noconfirm {
        args.push("--noconfirm");
    }
    for z in zsts {
        args.push(z.as_str());
    }

    let pacman = get_pacman();
    let command_str = format!("Running: sudo {} {}", pacman, args.join(" "));
    println!(
        "{} {} {}",
        info_icon(),
        pacman_badge(),
        prompt().apply_to(command_str.as_str())
    );
    let status = cmd(
        "sudo",
        [pacman]
            .into_iter()
            .chain(args.iter().copied())
            .collect::<Vec<_>>(),
    )
    .stderr_to_stdout()
    .run()?;
    if !status.status.success() {
        return Err(anyhow!("sudo {} -U failed", pacman));
    }
    Ok(())
}

pub fn install_repo_packages(repo: &[String], noconfirm: bool) -> Result<()> {
    if repo.is_empty() {
        return Ok(());
    }
    let mut args = vec!["-S"];
    if noconfirm {
        args.push("--noconfirm");
    }
    for r in repo {
        args.push(r.as_str());
    }

    let pacman = get_pacman();
    let command_str = format!("Running: sudo {} {}", pacman, args.join(" "));
    println!(
        "{} {} {}",
        info_icon(),
        pacman_badge(),
        prompt().apply_to(command_str.as_str())
    );
    let status = cmd(
        "sudo",
        [pacman]
            .into_iter()
            .chain(args.iter().copied())
            .collect::<Vec<_>>(),
    )
    .stderr_to_stdout()
    .run()?;
    if !status.status.success() {
        return Err(anyhow!("sudo {} -S (repo) failed", pacman));
    }
    Ok(())
}

pub fn sudo_pacman_scc() -> Result<()> {
    let pacman = get_pacman();
    let status = cmd("sudo", [pacman, "-Scc"]).stderr_to_stdout().run()?;
    if !status.status.success() {
        return Err(anyhow!("sudo {} -Scc failed", pacman));
    }
    Ok(())
}

pub async fn list_outdated_pacman_packages(
    forcerefresh: bool,
) -> Result<Vec<(String, String, String)>> {
    // pacman -Qu outputs: "package_name old_version -> new_version"
    // We need to get both old (installed) and new (available) versions
    //
    let pacman = get_pacman();
    let mut refresh_arg = String::from("-Sy");
    if forcerefresh {
        refresh_arg = String::from("-Syy")
    }
    let refresh_args = vec![refresh_arg];
    if !passthrough_to_pacman(&refresh_args).await? {
        return Ok(vec![]);
    }
    let out = task::spawn_blocking(move || {
        cmd("sudo", [pacman, "-Qu"])
            .stdout_capture()
            .stderr_null()
            .unchecked()
            .run()
    })
    .await??;

    if !out.status.success() {
        // Exit code 1 means no updates available, which is fine
        return Ok(vec![]);
    }

    let stdout = String::from_utf8_lossy(&out.stdout);
    let mut packages = vec![];

    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        // Format is: "package_name old_version -> new_version"
        if let Some((name_old, new_ver)) = line.split_once(" -> ") {
            if let Some((name, old_ver)) = name_old.split_once(' ') {
                packages.push((
                    name.to_string(),
                    old_ver.to_string(),
                    new_ver.trim().to_string(),
                ));
            }
        }
    }

    Ok(packages)
}
