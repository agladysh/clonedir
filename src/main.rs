use clap::Parser;

#[derive(Parser)]
#[command(name = "clonedir")]
#[command(about = "Clone a directory using copy-on-write where possible")]
struct Args {
    /// Source directory to clone
    from: String,
    /// Destination directory
    to: String,
}

fn main() {
    let args = Args::parse();
    clonedir_lib::clonedir(&args.from, &args.to).expect("Failed to clone directory");
}
