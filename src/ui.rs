
use anyhow::Result;
use dialoguer::MultiSelect;

#[derive(Debug, Clone)]
pub struct Pickable {
    pub name: String,
    pub current: String,
    pub latest: String,
}

pub fn pick_updates(items: &[Pickable]) -> Result<Vec<String>> {
    let items_disp: Vec<String> = items.iter().map(|p| {
        format!("{:<32} {:>12}  →  {:<12}", p.name, p.current, p.latest)
    }).collect();

    let selected = MultiSelect::new()
        .with_prompt("Select AUR packages to update")
        .items(&items_disp)
        .defaults(&vec![true; items.len()])
        .report(true)
        .interact()?;

    let mut out = vec![];
    for i in selected {
        out.push(items[i].name.clone());
    }
    Ok(out)
}

pub fn pick_updates_numeric(items: &[Pickable]) -> Result<Vec<String>> {
    // Print numbered list
    for (i, p) in items.iter().enumerate() {
        println!("{:>2}) {:<32} {:>12}  →  {:<12}", i + 1, p.name, p.current, p.latest);
    }
    print!("Enter numbers to update (e.g., 1 3 5), or empty to skip: ");
    use std::io::{self, Write};
    io::stdout().flush()?;
    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    let tokens = line
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|t| !t.is_empty());
    let mut out = vec![];
    for t in tokens {
        if let Ok(n) = t.parse::<usize>() {
            if n >= 1 && n <= items.len() {
                out.push(items[n - 1].name.clone());
            }
        }
    }
    Ok(out)
}
