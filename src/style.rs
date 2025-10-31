use console::Style;

// Status styles
pub fn success() -> Style {
    Style::new().green().bright()
}

pub fn error() -> Style {
    Style::new().red().bright()
}

pub fn warning() -> Style {
    Style::new().yellow().bright()
}

pub fn info() -> Style {
    Style::new().cyan()
}

// UI element styles
pub fn header() -> Style {
    Style::new().bold().underlined()
}

pub fn prompt() -> Style {
    Style::new().bold()
}

pub fn highlight() -> Style {
    Style::new().bold().yellow()
}

pub fn dim() -> Style {
    Style::new().dim()
}

// Package version styles
pub fn current_version() -> Style {
    Style::new().red()
}

pub fn new_version() -> Style {
    Style::new().green().bright()
}

// Package name style
pub fn package_name() -> Style {
    Style::new().bold()
}

// Command style
pub fn command() -> Style {
    Style::new().blue()
}

// Path style
pub fn path() -> Style {
    Style::new().cyan().italic()
}

// Number style
pub fn number() -> Style {
    Style::new().cyan()
}
