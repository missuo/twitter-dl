mod download;
mod downloader;
mod model;
mod twitter;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[clap(version)]
struct Args {
    #[clap(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Download tweets
    Download(DownloadArgs),
    /// Serve the downloaded tweet viewer
    Serve,
}

#[derive(Parser, Debug)]
pub struct DownloadArgs {
    /// Path to the authentication details file
    #[clap(short, long, default_value = "./auth.json")]
    auth: PathBuf,
    /// Where to save downloaded media (a sub folder will be created for each username)
    #[clap(short, long, default_value = "./")]
    out: PathBuf,
    /// Username(s) to download from (comma seperated)
    #[clap(short, long)]
    users: Option<String>,
    /// File containing list of usernames to download from (one per line)
    #[clap(short, long)]
    list: Option<PathBuf>,
    /// Download photos
    #[clap(long)]
    photos: bool,
    /// Download videos
    #[clap(long)]
    videos: bool,
    /// Download gifs
    #[clap(long)]
    gifs: bool,
    /// Rescan tweets that have already been loaded
    #[clap(long)]
    rescan: bool,
    /// Continue even if an account fails to download
    #[clap(long)]
    continue_on_error: bool,
    /// Use Twitter API 2 (Warning: Does not support Video and Gif downloads)
    #[clap(long)]
    api_v2: bool,
    /// Number of downloads to do concurrently
    #[clap(long, default_value_t = 4)]
    concurrency: usize,
    #[clap(arg_enum, default_value_t = FileExistsPolicy::Warn)]
    file_exists_policy: FileExistsPolicy,
}

#[derive(clap::ArgEnum, Debug, Clone, Eq, PartialEq)]
pub enum FileExistsPolicy {
    /// The existing file will be overwritten with a new download
    Overwrite,
    /// The data file will be updated to include the already present file
    Adopt,
    /// A warning is printed to the console
    Warn,
}

#[tokio::main]
async fn main() {
    if let Err(e) = main2().await {
        eprintln!("{:#}", e);
        std::process::exit(1);
    }
}

async fn main2() -> anyhow::Result<()> {
    let args: Args = Args::parse();
    match args.command {
        Commands::Download(args) => crate::download::download(args).await?,
        Commands::Serve => {}
    };
    Ok(())
}
