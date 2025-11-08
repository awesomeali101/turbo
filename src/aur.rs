use crate::build::AurSource;
use crate::config::Config;
use anyhow::{anyhow, Context, Result};
use petgraph::algo::toposort;
use petgraph::graph::DiGraph;
use petgraph::graph::NodeIndex;
use rayon::prelude::*;
use reqwest::blocking::Client;
use reqwest::StatusCode;
use serde::Deserialize;
use std::collections::{HashMap, HashSet, VecDeque};
use std::thread;
use std::time::Duration;

const GITHUB_SRCINFO_TIMEOUT_SECS: u64 = 45;
const GITHUB_SRCINFO_MAX_RETRIES: usize = 3;
const GITHUB_SRCINFO_RETRY_DELAY_SECS: u64 = 2;

#[derive(Debug, Deserialize)]
pub struct AurMeta {
    #[serde(rename = "resultcount")]
    pub resultcount: u32,
    pub results: Vec<AurInfo>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct AurInfo {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "PackageBase")]
    pub pkgbase: String,
    #[serde(rename = "Version")]
    pub version: String,
    #[serde(rename = "Depends")]
    pub depends: Option<Vec<String>>,
    #[serde(rename = "MakeDepends")]
    pub makedepends: Option<Vec<String>>,
    #[serde(rename = "CheckDepends")]
    pub checkdepends: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
pub struct AurRpcResponse {
    #[serde(rename = "type")]
    pub r#type: String,
    #[serde(flatten)]
    pub meta: AurMeta,
}

fn aur_rpc_info(client: &Client, names: &[String]) -> Result<AurMeta> {
    if names.is_empty() {
        return Ok(AurMeta {
            resultcount: 0,
            results: vec![],
        });
    }
    let mut url = String::from("https://aur.archlinux.org/rpc/?v=5&type=info");
    for n in names {
        url.push_str("&arg[]=");
        url.push_str(&urlencoding::encode(n));
    }
    let meta: AurMeta = client.get(&url).send()?.error_for_status()?.json()?;
    Ok(meta)
}

pub fn aur_info_batch(
    cfg: &Config,
    client: &Client,
    names: Vec<String>,
) -> Result<HashMap<String, AurInfo>> {
    let infos = fetch_infos(cfg, client, &names)?;
    let mut map = HashMap::new();
    for info in infos {
        map.insert(info.name.clone(), info);
    }
    Ok(map)
}

fn strip_version(dep: &str) -> String {
    // foo>=1.2 -> foo
    dep.split(|c| c == '<' || c == '>' || c == '=')
        .next()
        .unwrap_or(dep)
        .to_string()
}

fn resolve_dep_names(info: &AurInfo) -> Vec<String> {
    let mut out = vec![];
    if let Some(v) = &info.depends {
        out.extend(v.iter().map(|s| strip_version(s)));
    }
    if let Some(v) = &info.makedepends {
        out.extend(v.iter().map(|s| strip_version(s)));
    }
    if let Some(v) = &info.checkdepends {
        out.extend(v.iter().map(|s| strip_version(s)));
    }
    out
}

pub fn resolve_build_order(cfg: &Config, client: &Client, roots: &[String]) -> Result<Vec<String>> {
    // BFS fetch AUR info & dependencies, but only keep AUR packages (repo deps handled by pacman)
    let mut to_visit: Vec<String> = roots.to_vec();
    let mut seen: HashSet<String> = HashSet::new();
    let mut infos: HashMap<String, AurInfo> = HashMap::new();

    while !to_visit.is_empty() {
        let chunk_len = to_visit.len().min(100);
        let mut chunk: Vec<String> = to_visit.drain(..chunk_len).collect();
        chunk.retain(|name| !seen.contains(name));
        if chunk.is_empty() {
            continue;
        }

        let fetched = fetch_infos(cfg, client, &chunk)?;
        for info in fetched {
            let name = info.name.clone();
            if !seen.insert(name.clone()) {
                continue;
            }
            let deps = resolve_dep_names(&info);
            to_visit.extend(deps);
            infos.insert(name, info);
        }
    }

    // Build graph among AUR infos only
    let mut index: HashMap<String, NodeIndex> = HashMap::new();
    let mut g = DiGraph::<String, ()>::new();
    for name in infos.keys() {
        let idx = g.add_node(name.clone());
        index.insert(name.clone(), idx);
    }
    for (name, info) in &infos {
        let from = index.get(name).unwrap();
        for d in resolve_dep_names(info) {
            if let Some(to) = index.get(&d) {
                // Edge: dep -> pkg (so topo gives deps first)
                g.add_edge(*to, *from, ());
            }
        }
    }

    let order_idx =
        toposort(&g, None).map_err(|e| anyhow!("Dependency cycle involving {:?}", e.node_id()))?;
    let mut order = vec![];
    for idx in order_idx {
        let name = g.node_weight(idx).unwrap();
        if roots.contains(name) || infos.contains_key(name) {
            order.push(name.clone());
        }
    }
    Ok(order
        .into_iter()
        .filter(|n| infos.contains_key(n))
        .collect())
}

fn fetch_infos(cfg: &Config, client: &Client, names: &[String]) -> Result<Vec<AurInfo>> {
    if names.is_empty() {
        return Ok(vec![]);
    }
    let mut seen = HashSet::new();
    let mut unique = Vec::new();
    for name in names {
        if seen.insert(name.clone()) {
            unique.push(name.clone());
        }
    }
    match AurSource::from_cfg(cfg) {
        AurSource::Official => Ok(aur_rpc_info(client, &unique)?.results),
        AurSource::Github => github_fetch_infos(cfg, client, &unique),
    }
}

fn github_fetch_infos(cfg: &Config, client: &Client, names: &[String]) -> Result<Vec<AurInfo>> {
    if names.is_empty() {
        return Ok(vec![]);
    }
    let raw_base = github_raw_base(cfg)?;
    let mut queue: VecDeque<String> = VecDeque::from(names.to_vec());
    let mut attempts: HashMap<String, u8> = HashMap::new();
    let mut branch_cache: HashMap<String, Vec<AurInfo>> = HashMap::new();
    let mut package_to_branch: HashMap<String, String> = HashMap::new();
    let mut results: HashMap<String, AurInfo> = HashMap::new();

    while !queue.is_empty() {
        let chunk_len = queue.len().min(100);
        let mut chunk: Vec<String> = (0..chunk_len).filter_map(|_| queue.pop_front()).collect();
        chunk.retain(|pkg| !results.contains_key(pkg));
        if chunk.is_empty() {
            continue;
        }

        let mut branches_to_fetch: Vec<String> = chunk
            .iter()
            .map(|pkg| {
                package_to_branch
                    .get(pkg)
                    .cloned()
                    .unwrap_or_else(|| pkg.clone())
            })
            .filter(|branch| !branch_cache.contains_key(branch))
            .collect();
        branches_to_fetch.sort();
        branches_to_fetch.dedup();

        if !branches_to_fetch.is_empty() {
            let fetched = fetch_branches_parallel(client, &raw_base, &branches_to_fetch)?;
            for (branch, entries) in fetched {
                for info in &entries {
                    package_to_branch
                        .entry(info.name.clone())
                        .or_insert(info.pkgbase.clone());
                }
                branch_cache.insert(branch, entries);
            }
        }

        for pkg in chunk {
            if results.contains_key(&pkg) {
                continue;
            }
            let branch = package_to_branch
                .get(&pkg)
                .cloned()
                .unwrap_or_else(|| pkg.clone());
            if let Some(entries) = branch_cache.get(&branch) {
                if let Some(info) = entries.iter().find(|info| info.name == pkg) {
                    results.insert(pkg.clone(), info.clone());
                    continue;
                }
            }

            let entry = attempts.entry(pkg.clone()).or_insert(0);
            if *entry == 0 {
                *entry = 1;
                queue.push_back(pkg);
            }
        }
    }

    Ok(results.into_iter().map(|(_, v)| v).collect())
}

fn fetch_branches_parallel(
    client: &Client,
    raw_base: &str,
    branches: &[String],
) -> Result<Vec<(String, Vec<AurInfo>)>> {
    branches
        .par_iter()
        .map(|branch| {
            let infos = fetch_branch_srcinfo(client, raw_base, branch)
                .with_context(|| format!("Failed to fetch .SRCINFO for {}", branch))?;
            Ok((branch.clone(), infos))
        })
        .collect()
}

fn github_raw_base(cfg: &Config) -> Result<String> {
    let base = cfg
        .mirror_base
        .as_deref()
        .unwrap_or("https://github.com/archlinux/aur");
    let trimmed = base.trim();
    let trimmed = trimmed.trim_end_matches('/');
    let trimmed = trimmed.strip_suffix(".git").unwrap_or(trimmed);

    if let Some(rest) = trimmed.strip_prefix("https://github.com/") {
        return Ok(format!("https://raw.githubusercontent.com/{}", rest));
    }
    if let Some(rest) = trimmed.strip_prefix("http://github.com/") {
        return Ok(format!("https://raw.githubusercontent.com/{}", rest));
    }
    if let Some(rest) = trimmed.strip_prefix("git@github.com:") {
        return Ok(format!("https://raw.githubusercontent.com/{}", rest));
    }
    if let Some(rest) = trimmed.strip_prefix("ssh://git@github.com/") {
        return Ok(format!("https://raw.githubusercontent.com/{}", rest));
    }
    Err(anyhow!(
        "Unsupported GitHub mirror base '{}'; expected a github.com URL",
        base
    ))
}

fn fetch_branch_srcinfo(client: &Client, raw_base: &str, branch: &str) -> Result<Vec<AurInfo>> {
    let mut urls = vec![format!("{}/{}/.SRCINFO", raw_base, branch)];
    // Packages also exist as directories under the main branch; try common defaults.
    for default_branch in ["master", "main"] {
        urls.push(format!(
            "{}/{}/{}/.SRCINFO",
            raw_base, default_branch, branch
        ));
    }

    let mut last_err: Option<anyhow::Error> = None;
    for url in urls {
        match fetch_srcinfo_from_url(client, &url, branch) {
            Ok(Some(infos)) => return Ok(infos),
            Ok(None) => continue,
            Err(e) => {
                last_err = Some(e);
            }
        }
    }

    if let Some(err) = last_err {
        Err(err)
    } else {
        Ok(vec![])
    }
}

fn fetch_srcinfo_from_url(
    client: &Client,
    url: &str,
    pkgname: &str,
) -> Result<Option<Vec<AurInfo>>> {
    for attempt in 0..GITHUB_SRCINFO_MAX_RETRIES {
        let resp_result = client
            .get(url)
            .timeout(Duration::from_secs(GITHUB_SRCINFO_TIMEOUT_SECS))
            .send();

        match resp_result {
            Ok(resp) => {
                if resp.status() == StatusCode::NOT_FOUND {
                    return Ok(None);
                }
                let resp = resp.error_for_status().with_context(|| {
                    format!(
                        "GitHub mirror returned an error for {} while requesting {}",
                        pkgname, url
                    )
                })?;
                let text = resp
                    .text()
                    .with_context(|| format!("Failed to read .SRCINFO for {}", pkgname))?;
                let parsed = parse_srcinfo(&text)
                    .with_context(|| format!("Failed to parse .SRCINFO for {}", pkgname))?;
                return Ok(Some(parsed));
            }
            Err(err) => {
                let is_last = attempt + 1 == GITHUB_SRCINFO_MAX_RETRIES;
                if err.is_timeout() && !is_last {
                    thread::sleep(Duration::from_secs(GITHUB_SRCINFO_RETRY_DELAY_SECS));
                    continue;
                } else {
                    return Err(anyhow!(
                        "Failed to reach GitHub mirror for {} (attempt {} of {}): {}",
                        pkgname,
                        attempt + 1,
                        GITHUB_SRCINFO_MAX_RETRIES,
                        err
                    ));
                }
            }
        }
    }

    Ok(None)
}

#[derive(Default, Clone)]
struct DepFields {
    depends: Vec<String>,
    makedepends: Vec<String>,
    checkdepends: Vec<String>,
}

fn parse_srcinfo(contents: &str) -> Result<Vec<AurInfo>> {
    let mut pkgbase: Option<String> = None;
    let mut pkgver: Option<String> = None;
    let mut pkgrel: Option<String> = None;
    let mut epoch: Option<String> = None;
    let mut base_fields = DepFields::default();
    let mut pkg_fields: HashMap<String, DepFields> = HashMap::new();
    let mut pkg_names: Vec<String> = Vec::new();
    let mut current_pkg: Option<String> = None;

    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = match line.split_once('=') {
            Some((k, v)) => (k.trim(), v.trim()),
            None => continue,
        };
        match key {
            "pkgbase" => {
                pkgbase = Some(value.to_string());
                current_pkg = None;
            }
            "pkgver" => {
                pkgver = Some(value.to_string());
            }
            "pkgrel" => {
                pkgrel = Some(value.to_string());
            }
            "epoch" => {
                if !value.is_empty() {
                    epoch = Some(value.to_string());
                }
            }
            "pkgname" => {
                let name = value.to_string();
                current_pkg = Some(name.clone());
                pkg_fields.entry(name.clone()).or_default();
                pkg_names.push(name);
            }
            _ if key == "depends" || key.starts_with("depends_") => {
                let entry = value.to_string();
                if let Some(pkg) = &current_pkg {
                    pkg_fields
                        .entry(pkg.clone())
                        .or_default()
                        .depends
                        .push(entry);
                } else {
                    base_fields.depends.push(entry);
                }
            }
            _ if key == "makedepends" || key.starts_with("makedepends_") => {
                let entry = value.to_string();
                if let Some(pkg) = &current_pkg {
                    pkg_fields
                        .entry(pkg.clone())
                        .or_default()
                        .makedepends
                        .push(entry);
                } else {
                    base_fields.makedepends.push(entry);
                }
            }
            _ if key == "checkdepends" || key.starts_with("checkdepends_") => {
                let entry = value.to_string();
                if let Some(pkg) = &current_pkg {
                    pkg_fields
                        .entry(pkg.clone())
                        .or_default()
                        .checkdepends
                        .push(entry);
                } else {
                    base_fields.checkdepends.push(entry);
                }
            }
            _ => {}
        }
    }

    let pkgbase = pkgbase.ok_or_else(|| anyhow!("Missing pkgbase in .SRCINFO"))?;
    let pkgver = pkgver.ok_or_else(|| anyhow!("Missing pkgver in .SRCINFO for {}", pkgbase))?;
    let pkgrel = pkgrel.ok_or_else(|| anyhow!("Missing pkgrel in .SRCINFO for {}", pkgbase))?;
    if pkg_names.is_empty() {
        pkg_fields.entry(pkgbase.clone()).or_default();
        pkg_names.push(pkgbase.clone());
    }
    let version = format_version(epoch.as_deref(), &pkgver, &pkgrel);

    let mut infos = Vec::new();
    for name in pkg_names {
        let pkg_specific = pkg_fields.remove(&name).unwrap_or_default();
        let merged = merge_fields(&base_fields, &pkg_specific);
        infos.push(AurInfo {
            name: name.clone(),
            pkgbase: pkgbase.clone(),
            version: version.clone(),
            depends: vec_to_option(merged.depends),
            makedepends: vec_to_option(merged.makedepends),
            checkdepends: vec_to_option(merged.checkdepends),
        });
    }
    Ok(infos)
}

fn merge_fields(base: &DepFields, specific: &DepFields) -> DepFields {
    DepFields {
        depends: merge_lists(&base.depends, &specific.depends),
        makedepends: merge_lists(&base.makedepends, &specific.makedepends),
        checkdepends: merge_lists(&base.checkdepends, &specific.checkdepends),
    }
}

fn merge_lists(a: &[String], b: &[String]) -> Vec<String> {
    let mut out = Vec::with_capacity(a.len() + b.len());
    out.extend(a.iter().cloned());
    out.extend(b.iter().cloned());
    out
}

fn vec_to_option(v: Vec<String>) -> Option<Vec<String>> {
    if v.is_empty() {
        None
    } else {
        Some(v)
    }
}

fn format_version(epoch: Option<&str>, pkgver: &str, pkgrel: &str) -> String {
    match epoch {
        Some(e) if !e.is_empty() && e != "0" => format!("{}:{}-{}", e, pkgver, pkgrel),
        _ => format!("{}-{}", pkgver, pkgrel),
    }
}
