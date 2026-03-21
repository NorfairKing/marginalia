use clap::Parser;

#[derive(Parser)]
#[command(name = "marginalia")]
#[command(about = "Find [check] annotations in code comments near changed lines")]
pub struct Cli {
    /// Base branch to diff against (auto-detects main/master if not given).
    #[arg(long)]
    pub base: Option<String>,

    /// Output format.
    #[arg(long, default_value = "text")]
    pub format: OutputFormat,
}

#[derive(Clone, clap::ValueEnum)]
pub enum OutputFormat {
    Text,
    Json,
}

