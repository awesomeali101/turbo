use anyhow::{anyhow, Result};
use duct::cmd;
use std::collections::HashMap;

pub fn run_pacman(args: &[&str]) -> Result<()> {
    let status = cmd(
        "sudo",
        ["pacman"]
            .into_iter()
            .chain(args.iter().copied())
            .collect::<Vec<_>>(),
    )
    .stderr_to_stdout()
    .run()?;
    if !status.status.success() {
        return Err(anyhow!("pacman {:?} failed", args));
    }
    Ok(())
}

pub fn is_in_repo(name: &str) -> Result<bool> {
    let res = cmd(
        "bash",
        ["-lc", &format!("sudo pacman -Si -- {}", shell_escape(name))],
    )
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

pub fn list_foreign_packages() -> Result<HashMap<String, String>> {
    // pacman -Qm : foreign; we'll get name and version
    let out = cmd("sudo", ["pacman", "-Qm"]).stderr_to_stdout().read()?;
    let mut map = HashMap::new();
    for line in out.lines() {
        if let Some((n, v)) = line.split_once(' ') {
            map.insert(n.to_string(), v.to_string());
        }
    }
    Ok(map)
}

pub fn vercmp(a: &str, b: &str) -> Result<i32> {
    // pacman's vercmp prints -1, 0, or 1 on stdout
    let out = cmd("vercmp", [a, b]).stderr_to_stdout().read()?;
    let trimmed = out.trim();
    let v: i32 = trimmed
        .parse()
        .map_err(|_| anyhow!("invalid vercmp output: {}", trimmed))?;
    Ok(v)
}

pub fn split_repo_vs_aur(pkgs: &[String]) -> Result<(Vec<String>, Vec<String>)> {
    let mut repo = vec![];
    let mut aur = vec![];
    for p in pkgs {
        // If pacman -Si finds it in a repo, treat as repo; else assume AUR
        let res = cmd(
            "bash",
            ["-lc", &format!("sudo pacman -Si -- {}", shell_escape(p))],
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
    let status = cmd(
        "sudo",
        ["pacman"]
            .into_iter()
            .chain(args.iter().copied())
            .collect::<Vec<_>>(),
    )
    .stderr_to_stdout()
    .run()?;
    if !status.status.success() {
        return Err(anyhow!("sudo pacman -U failed"));
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
    let status = cmd(
        "sudo",
        ["pacman"]
            .into_iter()
            .chain(args.iter().copied())
            .collect::<Vec<_>>(),
    )
    .stderr_to_stdout()
    .run()?;
    if !status.status.success() {
        return Err(anyhow!("sudo pacman -S (repo) failed"));
    }
    Ok(())
}

pub fn sudo_pacman_scc() -> Result<()> {
    let status = cmd("sudo", ["pacman", "-Scc"]).stderr_to_stdout().run()?;
    if !status.status.success() {
        return Err(anyhow!("sudo pacman -Scc failed"));
    }
    Ok(())
}

pub fn list_outdated_pacman_packages() -> Result<Vec<(String, String, String)>> {
    // pacman -Qu outputs: "package_name old_version -> new_version"
    // We need to get both old (installed) and new (available) versions
    let out = cmd("pacman", ["-Qu"])
        .stdout_capture()
        .stderr_null()
        .unchecked()
        .run()?;

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
