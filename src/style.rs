use console::Style;

// Core status styles
pub fn success() -> Style {
    Style::new().green().bright().bold()
}

pub fn error() -> Style {
    Style::new().red().bright().bold()
}

pub fn warning() -> Style {
    Style::new().yellow().bold()
}

pub fn info() -> Style {
    Style::new().cyan().bright()
}

// Styled icons
pub fn success_icon() -> String {
    format!("{}", success().apply_to("✔"))
}

pub fn error_icon() -> String {
    format!("{}", error().apply_to("✘"))
}

pub fn warn_icon() -> String {
    format!("{}", warning().apply_to("⚠"))
}

pub fn info_icon() -> String {
    format!("{}", info().apply_to("ℹ"))
}

pub fn bullet() -> String {
    format!("{}", dim().apply_to("•"))
}

// UI element styles
pub fn section_title() -> Style {
    Style::new().bold().color256(44)
}

pub fn prompt() -> Style {
    Style::new().bold().color256(208)
}

pub fn highlight() -> Style {
    Style::new().bold().color256(214)
}

pub fn highlight_value() -> Style {
    Style::new().bold().color256(208)
}

pub fn dim() -> Style {
    Style::new().dim()
}

// Accent helpers
pub fn aur_accent() -> Style {
    Style::new().bold().magenta()
}

pub fn github_accent() -> Style {
    Style::new().bold().color256(177)
}

pub fn pacman_accent() -> Style {
    Style::new().bold().color256(81)
}

pub fn badge(label: &str, style: Style) -> String {
    let text = format!("[{}]", label);
    format!("{}", style.apply_to(text))
}

pub fn aur_badge() -> String {
    badge("AUR", aur_accent())
}

pub fn github_badge() -> String {
    badge("GITHUB", github_accent())
}

pub fn github_aur_mirror_badge() -> String {
    badge("GITHUB-AUR", github_accent())
}

pub fn pacman_badge() -> String {
    badge("PACMAN", pacman_accent())
}

// Package version styles
pub fn current_version() -> Style {
    Style::new().color256(196).bold()
}

pub fn new_version() -> Style {
    Style::new().color256(82).bold()
}

// Package name style
pub fn package_name() -> Style {
    Style::new().bold().color256(45)
}

// Command style
pub fn command() -> Style {
    Style::new().bold().color256(33)
}

// Path style
pub fn path() -> Style {
    Style::new().italic().color256(213)
}

// Number style
pub fn number() -> Style {
    Style::new().bold().color256(39)
}
