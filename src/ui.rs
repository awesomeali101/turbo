use anyhow::Result;
use dialoguer::MultiSelect;

use crate::style::*;

#[derive(Debug, Clone)]
pub struct Pickable {
    pub name: String,
    pub current: String,
    pub latest: String,
}

pub fn pick_updates(items: &[Pickable]) -> Result<Vec<String>> {
    let items_disp: Vec<String> = items
        .iter()
        .map(|p| {
            let name = package_name().apply_to(&p.name);
            let current = current_version().apply_to(&p.current);
            let arrow = dim().apply_to("→");
            let latest = new_version().apply_to(&p.latest);

            format!(
                "{} {name:<32} {current:>12}  {arrow}  {latest:<12}",
                bullet(),
                name = name,
                current = current,
                arrow = arrow,
                latest = latest
            )
        })
        .collect();

    let prompt_label = format!(
        "{} {}",
        info_icon(),
        prompt().apply_to("Select AUR packages to update")
    );
    let selected = MultiSelect::new()
        .with_prompt(prompt_label)
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
        let num = number().apply_to(format!("{:>2})", i + 1));
        let name = package_name().apply_to(&p.name);
        let current = current_version().apply_to(&p.current);
        let arrow = dim().apply_to("→");
        let latest = new_version().apply_to(&p.latest);

        println!(
            "{} {} {:<32} {:>12}  {}  {:<12}",
            bullet(),
            num,
            name,
            current,
            arrow,
            latest
        );
    }
    print!(
        "{} {} ",
        info_icon(),
        prompt().apply_to("Enter numbers to update (e.g., 1 3 5), or empty to skip:")
    );
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
