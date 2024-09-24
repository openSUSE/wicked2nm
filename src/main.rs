mod bond;
mod bridge;
mod infiniband;
mod interface;
mod migrate;
mod netconfig;
mod reader;
mod tuntap;
mod vlan;
mod wireless;

use clap::builder::TypedValueParser;
use clap::{Args, Parser, Subcommand};
use log::*;
use migrate::migrate;
use reader::read as wicked_read;
use serde::Serialize;
use simplelog::ConfigBuilder;
use std::process::{ExitCode, Termination};
use tokio::sync::OnceCell;

use crate::interface::Interface;
use crate::netconfig::Netconfig;

#[derive(Parser)]
#[command(name = "migrate-wicked", version(concat!(env!("CARGO_PKG_VERSION"),"~",env!("GIT_HEAD"))), about, long_about = None)]
struct Cli {
    #[clap(flatten)]
    global_opts: GlobalOpts,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Args)]
struct GlobalOpts {
    #[arg(long, global = true, default_value_t = LevelFilter::Info, value_parser = clap::builder::PossibleValuesParser::new(["TRACE", "DEBUG", "INFO", "WARN", "ERROR"]).map(|s| s.parse::<LevelFilter>().unwrap()),)]
    pub log_level: LevelFilter,

    #[arg(long, global = true, env = "MIGRATE_WICKED_WITHOUT_NETCONFIG")]
    pub without_netconfig: bool,

    #[arg(long, global = true, default_value_t = String::from("/etc/sysconfig/network/config"), env = "MIGRATE_WICKED_NETCONFIG_PATH")]
    pub netconfig_path: String,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Shows the current xml wicked configuration
    Show {
        /// Format output
        #[arg(value_enum, short, long, default_value_t = Format::Json)]
        format: Format,

        /// Wicked XML Files or directories where the wicked xml configs are located
        paths: Vec<String>,
    },
    /// Migrate wicked state at path
    Migrate {
        /// Wicked XML Files or directories where the wicked xml configs are located
        paths: Vec<String>,

        /// Continue migration if warnings are encountered
        #[arg(short, long, global = true, env = "MIGRATE_WICKED_CONTINUE_MIGRATION")]
        continue_migration: bool,

        /// Run migration without sending connections to NetworkManager (can be run without NetworkManager installed)
        #[arg(long, global = true, env = "MIGRATE_WICKED_DRY_RUN")]
        dry_run: bool,

        /// Activate connections immediately
        #[arg(long, global = true, env = "MIGRATE_WICKED_ACTIVATE_CONNECTIONS")]
        activate_connections: bool,
    },
}

/// Supported output formats
#[derive(clap::ValueEnum, Clone)]
pub enum Format {
    Json,
    PrettyJson,
    Yaml,
    Xml,
    Text,
}

async fn run_command(cli: Cli) -> anyhow::Result<()> {
    match cli.command {
        Commands::Show { paths, format } => {
            MIGRATION_SETTINGS
                .set(MigrationSettings {
                    continue_migration: true,
                    dry_run: false,
                    activate_connections: true,
                    with_netconfig: !cli.global_opts.without_netconfig,
                    netconfig_path: cli.global_opts.netconfig_path,
                })
                .expect("MIGRATION_SETTINGS was set too early");

            let interfaces_result = wicked_read(paths)?;

            #[derive(Debug, Serialize)]
            struct WickedConfig {
                interface: Vec<Interface>,
                netconfig: Option<Netconfig>,
            }
            let show_output = WickedConfig {
                interface: interfaces_result.interfaces,
                netconfig: interfaces_result.netconfig,
            };

            let output = match format {
                Format::Json => serde_json::to_string(&show_output)?,
                Format::PrettyJson => serde_json::to_string_pretty(&show_output)?,
                Format::Yaml => serde_yaml::to_string(&show_output)?,
                Format::Xml => quick_xml::se::to_string_with_root("wicked-config", &show_output)?,
                Format::Text => format!("{:?}", show_output),
            };
            println!("{}", output);
            Ok(())
        }
        Commands::Migrate {
            paths,
            continue_migration,
            dry_run,
            activate_connections,
        } => {
            MIGRATION_SETTINGS
                .set(MigrationSettings {
                    continue_migration,
                    dry_run,
                    activate_connections,
                    with_netconfig: !cli.global_opts.without_netconfig,
                    netconfig_path: cli.global_opts.netconfig_path,
                })
                .expect("MIGRATION_SETTINGS was set too early");

            log::debug!(
                "Running migration with MigrationSettings: {:#?}",
                MIGRATION_SETTINGS.get().unwrap()
            );

            match migrate(paths).await {
                Ok(()) => Ok(()),
                Err(e) => Err(anyhow::anyhow!("Migration failed: {}", e)),
            }
        }
    }
}

/// Represents the result of execution.
pub enum CliResult {
    /// Successful execution.
    Ok = 0,
    /// Something went wrong.
    Error = 1,
}

impl Termination for CliResult {
    fn report(self) -> ExitCode {
        ExitCode::from(self as u8)
    }
}

#[derive(Debug)]
struct MigrationSettings {
    continue_migration: bool,
    dry_run: bool,
    activate_connections: bool,
    with_netconfig: bool,
    netconfig_path: String,
}

impl Default for MigrationSettings {
    fn default() -> Self {
        MigrationSettings {
            continue_migration: false,
            dry_run: false,
            activate_connections: true,
            with_netconfig: false,
            netconfig_path: "".to_string(),
        }
    }
}

static MIGRATION_SETTINGS: OnceCell<MigrationSettings> = OnceCell::const_new();

#[tokio::main]
async fn main() -> CliResult {
    let cli = Cli::parse();

    let config = ConfigBuilder::new()
        .set_time_level(LevelFilter::Off)
        .add_filter_allow("migrate_wicked".to_string())
        .build();

    simplelog::TermLogger::init(
        cli.global_opts.log_level,
        config,
        simplelog::TerminalMode::Stderr,
        simplelog::ColorChoice::Auto,
    )
    .unwrap();

    if let Err(error) = run_command(cli).await {
        log::error!("{}", error);
        return CliResult::Error;
    }

    CliResult::Ok
}
