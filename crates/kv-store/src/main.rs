use clap::{Parser, Subcommand};
use orthrust_store::{MemStore, Store};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser)]
#[command(name = "orthrust-store")]
#[command(about = "A tiny key-value store CLI")]
struct Cli {
    /// Path to the store file
    #[arg(long, default_value = "orthrust.db", global = true)]
    file: PathBuf,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Get { key: String },
    Set { key: String, value: String },
    Delete { key: String },
}

fn load(path: &PathBuf) -> std::io::Result<MemStore> {
    match std::fs::read(path) {
        Ok(bytes) => MemStore::from_bytes(&bytes)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(MemStore::new()),
        Err(e) => Err(e),
    }
}

fn save(path: &PathBuf, store: &MemStore) -> std::io::Result<()> {
    std::fs::write(path, store.to_bytes())
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let mut store = match load(&cli.file) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error loading store: {e}");
            return ExitCode::FAILURE;
        }
    };

    let result: Result<(), ()> = match cli.command {
        Commands::Get { key } => match store.get(&key) {
            Ok(v) => {
                println!("{v}");
                Ok(())
            }
            Err(e) => {
                eprintln!("error: {e}");
                return ExitCode::FAILURE;
            }
        },
        Commands::Set { key, value } => {
            store.set(&key, value);
            println!("OK");
            Ok(())
        }
        Commands::Delete { key } => match store.delete(&key) {
            Ok(v) => {
                println!("{v}");
                Ok(())
            }
            Err(e) => {
                eprintln!("error: {e}");
                return ExitCode::FAILURE;
            }
        },
    };

    if result.is_ok() {
        if let Err(e) = save(&cli.file, &store) {
            eprintln!("error saving store: {e}");
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}
