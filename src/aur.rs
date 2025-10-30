
use anyhow::{anyhow, Context, Result};
use petgraph::graph::NodeIndex;
use petgraph::graph::DiGraph;
use petgraph::algo::toposort;
use reqwest::blocking::Client;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};

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
    if names.is_empty() { return Ok(AurMeta{resultcount:0, results: vec![]}); }
    let mut url = String::from("https://aur.archlinux.org/rpc/?v=5&type=info");
    for n in names {
        url.push_str("&arg[]=");
        url.push_str(&urlencoding::encode(n));
    }
    let meta: AurMeta = client.get(&url).send()?.error_for_status()?.json()?;
    Ok(meta)
}

pub fn aur_info_batch(client: &Client, names: Vec<String>) -> Result<HashMap<String, AurInfo>> {
    let meta = aur_rpc_info(client, &names)?;
    let mut map = HashMap::new();
    for info in meta.results {
        map.insert(info.name.clone(), info);
    }
    Ok(map)
}

fn strip_version(dep: &str) -> String {
    // foo>=1.2 -> foo
    dep.split(|c| c=='<' || c=='>' || c=='=').next().unwrap_or(dep).to_string()
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

pub fn resolve_build_order(client: &Client, roots: &[String]) -> Result<Vec<String>> {
    // BFS fetch AUR info & dependencies, but only keep AUR packages (repo deps will be handled by pacman during makepkg)
    let mut to_visit: Vec<String> = roots.to_vec();
    let mut seen: HashSet<String> = HashSet::new();
    let mut infos: HashMap<String, AurInfo> = HashMap::new();

    while !to_visit.is_empty() {
        let chunk: Vec<String> = to_visit.drain(..).take(100).collect();
        let meta = aur_rpc_info(client, &chunk)?;
        // Record infos we got back
        for info in meta.results {
            let name = info.name.clone();
            if !seen.insert(name.clone()) { continue; }
            let deps = resolve_dep_names(&info);
            // Add possible AUR deps to visit
            to_visit.extend(deps);
            infos.insert(name.clone(), info);
        }
    }

    // Build graph among AUR infos only
    let mut index: HashMap<String, petgraph::prelude::NodeIndex> = HashMap::new();
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

    let order_idx = toposort(&g, None).map_err(|e| anyhow!("Dependency cycle involving {:?}", e.node_id()))?;
    let mut order = vec![];
    for idx in order_idx {
        let name = g.node_weight(idx).unwrap();
        if roots.contains(name) || infos.contains_key(name) {
            order.push(name.clone());
        }
    }
    Ok(order.into_iter().filter(|n| infos.contains_key(n)).collect())
}
