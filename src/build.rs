use anyhow::{anyhow, Result};
use duct::cmd;
use std::fs;
use std::path::Path;

use crate::config::Config;
use crate::style::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AurSource {
    Official,
    Github,
}

impl AurSource {
    pub fn from_cfg(cfg: &Config) -> Self {
        if cfg.aur_mirror.eq_ignore_ascii_case("github") {
            AurSource::Github
        } else {
            AurSource::Official
        }
    }
}

#[derive(Clone, Debug)]
pub struct AurCloneSpec {
    pub pkgbase: String,
    pub source: AurSource,
}

fn run_git_command(args: &[&str], timeout_secs: u64) -> Result<bool> {
    let output = cmd(
        "timeout",
        [&format!("{}s", timeout_secs), "git"]
            .into_iter()
            .chain(args.iter().cloned()),
    )
    .stderr_to_stdout()
    .unchecked()
    .run();

    match output {
        Ok(output) => Ok(output.status.success()),
        Err(_) => Ok(false), // Timeout or other error
    }
}

pub fn clone_aur_pkgs(cfg: &Config, pkgs: &[AurCloneSpec], dest: &Path) -> Result<()> {
    fs::create_dir_all(dest)?;

    for spec in pkgs {
        let p = &spec.pkgbase;
        let target = dest.join(p);
        if target.exists() {
            continue;
        }

        match spec.source {
            AurSource::Github => {
                // For GitHub mirror, use shallow clone of the specific branch
                let base = cfg
                    .mirror_base
                    .as_deref()
                    .unwrap_or("https://github.com/archlinux/aur");
                let url = base.trim_end_matches('/');
                let cmd_display = format!(
                    "timeout 300s git clone --depth 1 --single-branch --branch {} {} '{}'",
                    p,
                    url,
                    target.display()
                );

                // Clone just the specific branch shallowly
                println!(
                    "{} {} Cloning {} from GitHub mirror",
                    info_icon(),
                    github_aur_mirror_badge(),
                    package_name().apply_to(p)
                );
                println!(
                    "  {} {}",
                    dim().apply_to("↳"),
                    command().apply_to(&cmd_display)
                );
                let success = run_git_command(
                    &[
                        "clone",
                        "--depth",
                        "1",
                        "--single-branch",
                        "--branch",
                        p,
                        url,
                        target.to_string_lossy().as_ref(),
                    ],
                    300, // 5 minute timeout
                )?;

                if !success {
                    return Err(anyhow!("Failed to clone package {} from GitHub mirror. The package might not exist or the mirror might be unavailable.", p));
                }
            }
            AurSource::Official => {
                // Standard AUR clone
                let url = format!("https://aur.archlinux.org/{}.git", p);
                let cmd_display = format!("git clone {} '{}'", url, target.display());
                println!(
                    "{} {} Cloning {} from AUR",
                    info_icon(),
                    aur_badge(),
                    package_name().apply_to(p)
                );
                println!(
                    "  {} {}",
                    dim().apply_to("↳"),
                    command().apply_to(&cmd_display)
                );
                let status = cmd("git", ["clone", &url, target.to_string_lossy().as_ref()])
                    .stderr_to_stdout()
                    .run()?;

                if !status.status.success() {
                    return Err(anyhow!("git clone failed for {}", p));
                }
            }
        }
    }
    Ok(())
}

pub fn open_file_manager(cfg: &Config, root: &Path) -> Result<()> {
    // Block until the FM exits
    let fm = &cfg.file_manager;
    let status = cmd(fm, [root.to_string_lossy().as_ref()])
        .stderr_to_stdout()
        .run()?;
    if !status.status.success() {
        return Err(anyhow!("{} exited with failure", fm));
    }
    Ok(())
}

pub fn regen_srcinfo(pkgdir: &Path) -> Result<()> {
    // Ensure .SRCINFO is regenerated after edits
    let sh = format!(
        "cd {} && makepkg --printsrcinfo > .SRCINFO",
        pkgdir.to_string_lossy()
    );
    let status = cmd("bash", ["-lc", &sh]).stderr_to_stdout().run()?;
    if !status.status.success() {
        return Err(anyhow!(
            "makepkg --printsrcinfo failed in {}",
            pkgdir.display()
        ));
    }
    Ok(())
}

pub fn makepkg_build(pkgdir: &Path) -> Result<()> {
    let sh = format!(
        "cd {} && makepkg -s -f --cleanbuild --noconfirm",
        pkgdir.to_string_lossy()
    );
    let status = cmd("bash", ["-lc", &sh]).stderr_to_stdout().run()?;
    if !status.status.success() {
        return Err(anyhow!("makepkg build failed in {}", pkgdir.display()));
    }
    Ok(())
}

pub fn collect_zsts(root: &Path) -> Result<Vec<String>> {
    let mut out = vec![];
    for entry in fs::read_dir(root)? {
        let path = entry?.path();
        if path.is_dir() {
            // Search for *.pkg.tar.zst in subtrees
            for art in globwalk::GlobWalkerBuilder::from_patterns(&path, &["**/*.pkg.tar.zst"])
                .build()?
                .filter_map(Result::ok)
            {
                out.push(art.path().to_string_lossy().into_owned());
            }
        }
    }
    Ok(out)
}

pub fn verify_sources(pkgdir: &Path) -> Result<()> {
    // Verify and fetch sources and signatures before heavy build
    let sh = format!(
        "cd {} && makepkg --verifysource --noconfirm",
        pkgdir.to_string_lossy()
    );
    let status = cmd("bash", ["-lc", &sh]).stderr_to_stdout().run()?;
    if !status.status.success() {
        return Err(anyhow!(
            "makepkg --verifysource failed in {}",
            pkgdir.display()
        ));
    }
    Ok(())
}

pub fn import_validpgpkeys(pkgdir: &Path) -> Result<()> {
    let sh = format!(
        "cd {} && set -a; source PKGBUILD >/dev/null 2>&1 || true; for k in \"${{validpgpkeys[@]}}\"; do echo $k; done",
        pkgdir.to_string_lossy()
    );
    let out = cmd("bash", ["-lc", &sh]).stderr_to_stdout().read()?;
    let mut keys: Vec<&str> = vec![];
    for line in out.lines() {
        let t = line.trim();
        if !t.is_empty() {
            keys.push(t);
        }
    }
    if keys.is_empty() {
        return Ok(());
    }
    let servers = [
        "hkps://keys.openpgp.org",
        "hkps://keyserver.ubuntu.com",
        "hkps://keys.mailvelope.com",
    ];
    let mut last_err: Option<anyhow::Error> = None;
    for srv in &servers {
        let mut args: Vec<&str> = vec!["--keyserver", srv, "--recv-keys"];
        for k in &keys {
            args.push(k);
        }
        let res = cmd("gpg", args).stderr_to_stdout().run();
        match res {
            Ok(st) if st.status.success() => {
                return Ok(());
            }
            Ok(st) => {
                last_err = Some(anyhow!(
                    "gpg recv from {} failed: status {}",
                    srv,
                    st.status
                ));
            }
            Err(e) => {
                last_err = Some(anyhow!("gpg recv from {} failed: {}", srv, e));
            }
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow!("gpg --recv-keys failed")))
}

pub fn ensure_persistent_dirs(cfg: &Config) -> Result<()> {
    fs::create_dir_all(cfg.temp_dir())?;
    Ok(())
}

pub fn clean_dir_contents(dir: &Path) -> Result<()> {
    if dir.exists() {
        for entry in fs::read_dir(dir)? {
            let p = entry?.path();
            if p.is_dir() {
                fs::remove_dir_all(&p)?;
            } else {
                fs::remove_file(&p)?;
            }
        }
    }
    Ok(())
}

pub fn clean_cache(cfg: &Config) -> Result<()> {
    fs::create_dir_all(cfg.cache_dir())?;
    clean_dir_contents(&cfg.cache_dir())
}
