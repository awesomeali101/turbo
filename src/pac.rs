use anyhow::{anyhow, Context, Result};
use duct::cmd;
use std::collections::HashMap;
use std::process::Stdio;

pub fn run_pacman(args: &[&str]) -> Result<()> {
    let status = cmd("sudo", ["pacman"].into_iter().chain(args.iter().copied()).collect::<Vec<_>>())
        .stderr_to_stdout()
        .run()?;
    if !status.status.success() {
        return Err(anyhow!("pacman {:?} failed", args));
    }
    Ok(())
}

pub fn is_in_repo(name: &str) -> Result<bool> {
    let res = cmd("bash", ["-lc", &format!("sudo pacman -Si -- {}", shell_escape(name))])
        .stdout_capture()
        .stderr_null()
        .unchecked()
        .run()?;
    let ok = res.status.success() && !String::from_utf8_lossy(&res.stdout).is_empty();
    Ok(ok)
}

pub fn passthrough_to_pacman(args: &[String]) -> Result<()> {
    let mut full = vec![];
    for a in args {
        full.push(a.as_str());
    }
    run_pacman(&full)
}

pub fn list_foreign_packages() -> Result<HashMap<String,String>> {
    // pacman -Qm : foreign; we'll get name and version
    let out = cmd("sudo", ["pacman", "-Qm"]).stderr_to_stdout().read()?;
    let mut map = HashMap::new();
    for line in out.lines() {
        if let Some((n,v)) = line.split_once(' ') {
            map.insert(n.to_string(), v.to_string());
        }
    }
    Ok(map)
}

pub fn vercmp(a: &str, b: &str) -> Result<i32> {
    // pacman's vercmp prints -1, 0, or 1 on stdout
    let out = cmd("vercmp", [a, b]).stderr_to_stdout().read()?;
    let trimmed = out.trim();
    let v: i32 = trimmed.parse().map_err(|_| anyhow!("invalid vercmp output: {}", trimmed))?;
    Ok(v)
}

pub fn split_repo_vs_aur(pkgs: &[String]) -> Result<(Vec<String>, Vec<String>)> {
    let mut repo = vec![];
    let mut aur = vec![];
    for p in pkgs {
        // If pacman -Si finds it in a repo, treat as repo; else assume AUR
        let res = cmd("bash", ["-lc", &format!("sudo pacman -Si -- {}", shell_escape(p))])
            .stdout_capture()
            .stderr_null()
            .unchecked()
            .run()?;
        let ok = res.status.success() && !String::from_utf8_lossy(&res.stdout).is_empty();
        if ok { repo.push(p.clone()); } else { aur.push(p.clone()); }
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
    let status = cmd("sudo", ["pacman"].into_iter().chain(args.iter().copied()).collect::<Vec<_>>())
        .stderr_to_stdout()
        .run()?;
    if !status.status.success() {
        return Err(anyhow!("sudo pacman -U failed"));
    }
    Ok(())
}

pub fn sudo_pacman_U_with_repo(repo: &[String], zsts: &[String]) -> Result<()> {
    sudo_pacman_U_with_repo_inner(repo, zsts, false)
}

pub fn sudo_pacman_U_with_repo_noconfirm(repo: &[String], zsts: &[String]) -> Result<()> {
    sudo_pacman_U_with_repo_inner(repo, zsts, true)
}

fn sudo_pacman_U_with_repo_inner(repo: &[String], zsts: &[String], noconfirm: bool) -> Result<()> {
    // Install repo packages first (resolve deps), then single -U for all built AUR
    if !repo.is_empty() {
        let mut args = vec!["-S"];
        if noconfirm {
            args.push("--noconfirm");
        }
        for r in repo { args.push(r.as_str()); }
        let status = cmd("sudo", ["pacman"].into_iter().chain(args.iter().copied()).collect::<Vec<_>>())
            .stderr_to_stdout()
            .run()?;
        if !status.status.success() {
            return Err(anyhow!("sudo pacman -S (repo) failed"));
        }
    }
    if noconfirm {
        sudo_pacman_U_noconfirm(zsts)
    } else {
        sudo_pacman_U(zsts)
    }
}

pub fn sudo_pacman_scc() -> Result<()> {
    let status = cmd("sudo", ["pacman", "-Scc"]).stderr_to_stdout().run()?;
    if !status.status.success() {
        return Err(anyhow!("sudo pacman -Scc failed"));
    }
    Ok(())
}
