
use anyhow::{anyhow, Context, Result};
use duct::cmd;
use std::fs;
use std::path::{Path, PathBuf};

use crate::config::Config;

pub fn clone_aur_pkgs(cfg: &Config, pkgs: &[String], dest: &Path) -> Result<()> {
    fs::create_dir_all(dest)?;
    for p in pkgs {
        let url = if cfg.aur_mirror.to_lowercase() == "github" {
            let base = cfg
                .mirror_base
                .as_deref()
                .unwrap_or("https://github.com/archlinux-aur");
            format!("{}/{}.git", base.trim_end_matches('/'), p)
        } else {
            format!("https://aur.archlinux.org/{}.git", p)
        };
        let target = dest.join(p);
        if target.exists() {
            continue;
        }
        let status = cmd("git", ["clone", &url, target.to_string_lossy().as_ref()])
            .stderr_to_stdout()
            .run()?;
        if !status.status.success() {
            return Err(anyhow!("git clone failed for {}", p));
        }
    }
    Ok(())
}

pub fn open_file_manager(cfg: &Config, root: &Path) -> Result<()> {
    // Block until the FM exits
    let fm = &cfg.file_manager;
    let status = cmd(fm, [root.to_string_lossy().as_ref()]).stderr_to_stdout().run()?;
    if !status.status.success() {
        return Err(anyhow!("{} exited with failure", fm));
    }
    Ok(())
}

pub fn regen_srcinfo(pkgdir: &Path) -> Result<()> {
    // Ensure .SRCINFO is regenerated after edits
    let sh = format!("cd {} && makepkg --printsrcinfo > .SRCINFO", pkgdir.to_string_lossy());
    let status = cmd("bash", ["-lc", &sh]).stderr_to_stdout().run()?;
    if !status.status.success() {
        return Err(anyhow!("makepkg --printsrcinfo failed in {}", pkgdir.display()));
    }
    Ok(())
}

pub fn makepkg_build(pkgdir: &Path) -> Result<()> {
    let sh = format!("cd {} && makepkg -s -f --cleanbuild --noconfirm", pkgdir.to_string_lossy());
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
            for art in globwalk::GlobWalkerBuilder::from_patterns(&path, &["**/*.pkg.tar.zst"]).build()?.filter_map(Result::ok) {
                out.push(art.path().to_string_lossy().into_owned());
            }
        }
    }
    Ok(out)
}

pub fn verify_sources(pkgdir: &Path) -> Result<()> {
    // Verify and fetch sources and signatures before heavy build
    let sh = format!("cd {} && makepkg --verifysource --noconfirm", pkgdir.to_string_lossy());
    let status = cmd("bash", ["-lc", &sh]).stderr_to_stdout().run()?;
    if !status.status.success() {
        return Err(anyhow!("makepkg --verifysource failed in {}", pkgdir.display()));
    }
    Ok(())
}

pub fn import_validpgpkeys(pkgdir: &Path) -> Result<()> {
    // Use bash to source PKGBUILD and print validpgpkeys array, then import via gpg
    let sh = format!(
        "cd {} && set -a; source PKGBUILD >/dev/null 2>&1 || true; for k in \"${{validpgpkeys[@]}}\"; do echo $k; done",
        pkgdir.to_string_lossy()
    );
    let out = cmd("bash", ["-lc", &sh]).stderr_to_stdout().read()?;
    let mut keys: Vec<&str> = vec![];
    for line in out.lines() {
        let t = line.trim();
        if !t.is_empty() { keys.push(t); }
    }
    if keys.is_empty() {
        return Ok(());
    }
    let mut args: Vec<&str> = vec!["--keyserver", "hkps://keys.openpgp.org", "--recv-keys"];
    for k in &keys { args.push(k); }
    let status = cmd("gpg", args).stderr_to_stdout().run()?;
    if !status.status.success() {
        return Err(anyhow!("gpg --recv-keys failed for some keys in {}", pkgdir.display()));
    }
    Ok(())
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

