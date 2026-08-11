use std::process::ExitCode;

use lazycargo::args::help_text;
use lazycargo::{parse_args, AppConfig, CliAction};

fn main() -> ExitCode {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.is_empty() {
        return match lazycargo::ui::run() {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("error: {error}");
                ExitCode::from(1)
            }
        };
    }

    match parse_args(args) {
        Ok(CliAction::PrintHelp) => {
            print!("{}", help_text());
            ExitCode::SUCCESS
        }
        Ok(CliAction::Run(task)) => {
            println!("{}", task.to_command().display());
            ExitCode::SUCCESS
        }
        Ok(CliAction::SearchCrates(query)) => {
            println!(
                "search crates.io for '{}' (limit {})",
                query.query, query.limit
            );
            ExitCode::SUCCESS
        }
        Ok(CliAction::AddDependency(plan)) => {
            println!("{}", plan.to_command().display());
            ExitCode::SUCCESS
        }
        Ok(CliAction::PrintConfig) => match AppConfig::load_or_create() {
            Ok(config) => {
                println!("path: {}", AppConfig::config_path().display());
                match serde_json::to_string_pretty(&config) {
                    Ok(text) => println!("{text}"),
                    Err(error) => eprintln!("error: failed to render config: {error}"),
                }
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("error: failed to load config: {error}");
                ExitCode::from(1)
            }
        },
        Err(error) => {
            eprintln!("error: {error}");
            eprintln!();
            eprint!("{}", help_text());
            ExitCode::from(2)
        }
    }
}
