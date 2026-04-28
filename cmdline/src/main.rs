use clap::Parser;

#[derive(Debug, Parser)]
#[command(name = "pandora")]
#[command(about = "CLI entrypoint for Pandora", long_about = None)]
struct Cli {
    /// Optional name to greet.
    #[arg(short, long)]
    name: Option<String>,
}

fn main() {
    let cli = Cli::parse();

    match cli.name {
        Some(name) => println!("Hello, {name}!"),
        None => println!("Hello from Pandora CLI!"),
    }
}
