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
    /// Retrieve a value for a key
    Get { key: String },
    /// Store a key-value pair
    Set { key: String, value: String },
    /// Delete a key-value pair
    Delete { key: String },
    /// Check if a key exists
    Exists { key: String },
    /// List all keys in lexicographical order
    Keys,
    /// Clear all keys from the store
    Clear,
    /// Scan keys with optional prefix
    Scan {
        #[arg(default_value = "")]
        prefix: String,
    },
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
                Err(())
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
                Err(())
            }
        },
        Commands::Exists { key } => {
            if store.contains(&key) {
                println!("true");
                Ok(())
            } else {
                println!("false");
                Err(())
            }
        }
        Commands::Keys => {
            for key in store.keys() {
                println!("{key}");
            }
            Ok(())
        }
        Commands::Clear => {
            store.clear();
            println!("OK");
            Ok(())
        }
        Commands::Scan { prefix } => {
            for (k, v) in store.scan(&prefix) {
                println!("{k}={v}");
            }
            Ok(())
        }
    };

    if result.is_ok()
        && let Err(e) = save(&cli.file, &store)
    {
        eprintln!("error saving store: {e}");
        return ExitCode::FAILURE;
    }

    if result.is_ok() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
