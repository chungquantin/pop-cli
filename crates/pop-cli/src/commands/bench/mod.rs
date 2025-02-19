// SPDX-License-Identifier: GPL-3.0

use crate::{
	cli::{self},
	common::prompt::display_message,
};
use clap::{Args, Subcommand};
use frame_benchmarking_cli::PalletCmd;
use pallet::BenchmarkPallet;
use pop_parachains::run_pallet_benchmarking;

mod pallet;

/// Arguments for benchmarking a project.
#[derive(Args)]
#[command(args_conflicts_with_subcommands = true, ignore_errors = true)]
pub struct BenchmarkArgs {
	#[command(subcommand)]
	pub command: Command,

	/// How to construct the genesis state. Uses `none` by default.
	#[arg(long, alias = "genesis-builder-policy", hide = true)]
	genesis_builder: Option<String>,

	/// How to construct the genesis state. Uses `none` by default.
	#[arg(short, long = "skip", alias = "s", hide = true, action = clap::ArgAction::SetFalse)]
	skip_menu: bool,
}

/// Benchmark a pallet or a parachain.
#[derive(Subcommand)]
pub enum Command {
	/// Benchmark the extrinsic weight of FRAME Pallets
	#[clap(alias = "p", disable_help_flag = true)]
	Pallet(PalletCmd),
}

impl Command {
	/// Executes the command.
	pub(crate) fn execute(args: BenchmarkArgs) -> anyhow::Result<()> {
		let mut cli = cli::Cli;

		match args.command {
			Command::Pallet(mut cmd) => {
				if cmd.list.is_some() || cmd.json_output {
					if let Err(e) = run_pallet_benchmarking(&cmd) {
						return display_message(&e.to_string(), false, &mut cli);
					}
				}
				BenchmarkPallet { genesis_builder: args.genesis_builder, skip_menu: args.skip_menu }
					.execute(&mut cmd, &mut cli)
			},
		}
	}
}
