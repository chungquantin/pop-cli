// SPDX-License-Identifier: GPL-3.0

use crate::{
	cli::{self},
	common::prompt::display_message,
};
use clap::{Args, Subcommand};
use pallet::BenchmarkPallet;

mod overhead;
mod pallet;

/// Arguments for benchmarking a project.
#[derive(Args)]
#[command(args_conflicts_with_subcommands = true)]
pub struct BenchmarkArgs {
	#[command(subcommand)]
	pub command: Command,
}

/// Benchmark a pallet or a parachain.
#[derive(Subcommand)]
pub enum Command {
	/// Benchmark the extrinsic weight of FRAME Pallets
	#[clap(alias = "p")]
	Pallet(BenchmarkPallet),
	#[clap(alias = "o")]
	Overhead(overhead::BenchmarkOverhead),
}

impl Command {
	/// Executes the command.
	pub(crate) async fn execute(args: BenchmarkArgs) -> anyhow::Result<()> {
		let mut cli = cli::Cli;
		match args.command {
			Command::Pallet(mut cmd) => cmd.execute(&mut cli).await,
			Command::Overhead(mut cmd) => cmd.execute(&mut cli),
		}
	}
}
