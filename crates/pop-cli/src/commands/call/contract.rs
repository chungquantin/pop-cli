// SPDX-License-Identifier: GPL-3.0

use crate::{
	cli::{self, traits::*},
	common::build::has_contract_been_built,
};
use anyhow::{anyhow, Result};
use clap::Args;
use cliclack::spinner;
use pop_contracts::{
	build_smart_contract, call_smart_contract, dry_run_call, dry_run_gas_estimate_call,
	get_messages, parse_account, set_up_call, CallOpts, Verbosity,
};
use sp_weights::Weight;
use std::path::PathBuf;

const DEFAULT_URL: &str = "ws://localhost:9944/";
const DEFAULT_URI: &str = "//Alice";
const DEFAULT_PAYABLE_VALUE: &str = "0";

#[derive(Args, Clone)]
pub struct CallContractCommand {
	/// Path to the contract build directory.
	#[arg(short = 'p', long)]
	path: Option<PathBuf>,
	/// The address of the contract to call.
	#[clap(name = "contract", short = 'c', long, env = "CONTRACT")]
	contract: Option<String>,
	/// The name of the contract message to call.
	#[clap(long, short)]
	message: Option<String>,
	/// The constructor arguments, encoded as strings.
	#[clap(long, num_args = 0..,)]
	args: Vec<String>,
	/// The value to be transferred as part of the call.
	#[clap(name = "value", short = 'v', long, default_value = DEFAULT_PAYABLE_VALUE)]
	value: String,
	/// Maximum amount of gas to be used for this command.
	/// If not specified it will perform a dry-run to estimate the gas consumed for the
	/// call.
	#[clap(name = "gas", short = 'g', long)]
	gas_limit: Option<u64>,
	/// Maximum proof size for this command.
	/// If not specified it will perform a dry-run to estimate the proof size required.
	#[clap(short = 'P', long)]
	proof_size: Option<u64>,
	/// Websocket endpoint of a node.
	#[clap(name = "url", short = 'u', long, value_parser, default_value = DEFAULT_URL)]
	url: url::Url,
	/// Secret key URI for the account calling the contract.
	///
	/// e.g.
	/// - for a dev account "//Alice"
	/// - with a password "//Alice///SECRET_PASSWORD"
	#[clap(name = "suri", long, short, default_value = DEFAULT_URI)]
	suri: String,
	/// Submit an extrinsic for on-chain execution.
	#[clap(short('x'), long)]
	execute: bool,
	/// Perform a dry-run via RPC to estimate the gas usage. This does not submit a transaction.
	#[clap(short = 'D', long, conflicts_with = "execute")]
	dry_run: bool,
	/// Enables developer mode, bypassing certain user prompts for faster testing.
	/// Recommended for testing and local development only.
	#[clap(name = "dev", long, short, default_value = "false")]
	dev_mode: bool,
}
impl CallContractCommand {
	/// Executes the command.
	pub(crate) async fn execute(mut self) -> Result<()> {
		// Ensure contract is built.
		self.ensure_contract_built(&mut cli::Cli).await?;
		// Check if message specified via command line argument.
		let prompt_to_repeat_call = self.message.is_none();
		// Configure the call based on command line arguments/call UI.
		if let Err(e) = self.configure(&mut cli::Cli, false).await {
			display_message(&e.to_string(), false, &mut cli::Cli)?;
			return Ok(());
		};
		// Finally execute the call.
		if let Err(e) = self.execute_call(&mut cli::Cli, prompt_to_repeat_call).await {
			display_message(&e.to_string(), false, &mut cli::Cli)?;
		}
		Ok(())
	}

	fn display(&self) -> String {
		let mut full_message = "pop call contract".to_string();
		if let Some(path) = &self.path {
			full_message.push_str(&format!(" --path {}", path.display()));
		}
		if let Some(contract) = &self.contract {
			full_message.push_str(&format!(" --contract {}", contract));
		}
		if let Some(message) = &self.message {
			full_message.push_str(&format!(" --message {}", message));
		}
		if !self.args.is_empty() {
			full_message.push_str(&format!(" --args {}", self.args.join(",")));
		}
		if self.value != DEFAULT_PAYABLE_VALUE {
			full_message.push_str(&format!(" --value {}", self.value));
		}
		if let Some(gas_limit) = self.gas_limit {
			full_message.push_str(&format!(" --gas {}", gas_limit));
		}
		if let Some(proof_size) = self.proof_size {
			full_message.push_str(&format!(" --proof_size {}", proof_size));
		}
		full_message.push_str(&format!(" --url {} --suri {}", self.url, self.suri));
		if self.execute {
			full_message.push_str(" --execute");
		}
		if self.dry_run {
			full_message.push_str(" --dry_run");
		}
		full_message
	}

	/// Checks if the contract has been built; if not, builds it.
	async fn ensure_contract_built(&self, cli: &mut impl Cli) -> Result<()> {
		// Check if build exists in the specified "Contract build directory"
		if !has_contract_been_built(self.path.as_deref()) {
			// Build the contract in release mode
			cli.warning("NOTE: contract has not yet been built.")?;
			let spinner = spinner();
			spinner.start("Building contract in RELEASE mode...");
			let result = match build_smart_contract(self.path.as_deref(), true, Verbosity::Quiet) {
				Ok(result) => result,
				Err(e) => {
					return Err(anyhow!(format!(
						"🚫 An error occurred building your contract: {}\nUse `pop build` to retry with build output.",
						e.to_string()
					)));
				},
			};
			spinner.stop(format!(
				"Your contract artifacts are ready. You can find them in: {}",
				result.target_directory.display()
			));
		}
		Ok(())
	}

	/// Configure the call based on command line arguments/call UI.
	async fn configure(&mut self, cli: &mut impl Cli, repeat: bool) -> Result<()> {
		// Show intro on first run.
		if !repeat {
			cli.intro("Call a contract")?;
		}

		// If message has been specified via command line arguments, return early.
		if self.message.is_some() {
			return Ok(());
		}

		// Resolve path.
		let contract_path = match self.path.as_ref() {
			None => {
				let path = Some(PathBuf::from("./"));
				if has_contract_been_built(path.as_deref()) {
					self.path = path;
				} else {
					// Prompt for path.
					let input_path: String = cli
						.input("Where is your project located?")
						.placeholder("./")
						.default_input("./")
						.interact()?;
					self.path = Some(PathBuf::from(input_path));
				}

				self.path.as_ref().unwrap()
			},
			Some(p) => p,
		};

		// Parse the contract metadata provided. If there is an error, do not prompt for more.
		let messages = match get_messages(&contract_path) {
			Ok(messages) => messages,
			Err(e) => {
				return Err(anyhow!(format!(
					"Unable to fetch contract metadata: {}",
					e.to_string().replace("Anyhow error: ", "")
				)));
			},
		};

		// Resolve url.
		if !repeat && self.url.as_str() == DEFAULT_URL {
			// Prompt for url.
			let url: String = cli
				.input("Where is your contract deployed?")
				.placeholder("ws://localhost:9944")
				.default_input("ws://localhost:9944")
				.interact()?;
			self.url = url::Url::parse(&url)?
		};

		// Resolve contract address.
		if let None = self.contract {
			// Prompt for contract address.
			let contract_address: String = cli
				.input("Paste the on-chain contract address:")
				.placeholder("e.g. 5DYs7UGBm2LuX4ryvyqfksozNAW5V47tPbGiVgnjYWCZ29bt")
				.validate(|input: &String| match parse_account(input) {
					Ok(_) => Ok(()),
					Err(_) => Err("Invalid address."),
				})
				.default_input("5DYs7UGBm2LuX4ryvyqfksozNAW5V47tPbGiVgnjYWCZ29bt")
				.interact()?;
			self.contract = Some(contract_address);
		};

		// Resolve message.
		let message = {
			let mut prompt = cli.select("Select the message to call:");
			for select_message in &messages {
				prompt = prompt.item(
					select_message,
					format!("{}\n", &select_message.label),
					&select_message.docs,
				);
			}
			let message = prompt.interact()?;
			self.message = Some(message.label.clone());
			message
		};

		// Resolve message arguments.
		let mut contract_args = Vec::new();
		for arg in &message.args {
			let mut input = cli
				.input(format!("Enter the value for the parameter: {}", arg.label))
				.placeholder(&format!("Type required: {}", arg.type_name));

			// Set default input only if the parameter type is `Option` (Not mandatory)
			if arg.type_name == "Option" {
				input = input.default_input("");
			}
			contract_args.push(input.interact()?);
		}
		self.args = contract_args;

		// Resolve value.
		if message.payable && self.value == DEFAULT_PAYABLE_VALUE {
			self.value = cli
				.input("Value to transfer to the call:")
				.placeholder("0")
				.default_input("0")
				.validate(|input: &String| match input.parse::<u64>() {
					Ok(_) => Ok(()),
					Err(_) => Err("Invalid value."),
				})
				.interact()?;
		}

		// Resolve gas limit.
		if message.mutates && !self.dev_mode && self.gas_limit.is_none() {
			// Prompt for gas limit and proof_size of the call.
			let gas_limit_input: String = cli
				.input("Enter the gas limit:")
				.required(false)
				.default_input("")
				.placeholder("If left blank, an estimation will be used")
				.interact()?;
			self.gas_limit = gas_limit_input.parse::<u64>().ok(); // If blank or bad input, estimate it.
		}

		// Resolve proof size.
		if message.mutates && !self.dev_mode && self.proof_size.is_none() {
			let proof_size_input: String = cli
				.input("Enter the proof size limit:")
				.required(false)
				.placeholder("If left blank, an estimation will be used")
				.default_input("")
				.interact()?;
			self.proof_size = proof_size_input.parse::<u64>().ok(); // If blank or bad input, estimate it.
		}

		// Resolve who is calling the contract.
		if self.suri == DEFAULT_URI {
			// Prompt for uri.
			self.suri = cli
				.input("Signer calling the contract:")
				.placeholder("//Alice")
				.default_input("//Alice")
				.interact()?;
		};

		// Finally prompt for confirmation.
		let is_call_confirmed = if message.mutates && !self.dev_mode {
			cli.confirm("Do you want to execute the call? (Selecting 'No' will perform a dry run)")
				.initial_value(true)
				.interact()?
		} else {
			true
		};
		self.execute = is_call_confirmed && message.mutates;
		self.dry_run = !is_call_confirmed;

		cli.info(self.display())?;
		Ok(())
	}

	/// Execute the call.
	async fn execute_call(
		&mut self,
		cli: &mut impl Cli,
		prompt_to_repeat_call: bool,
	) -> Result<()> {
		let message = match &self.message {
			Some(message) => message.to_string(),
			None => {
				return Err(anyhow!("Please specify the message to call."));
			},
		};
		let contract = match &self.contract {
			Some(contract) => contract.to_string(),
			None => {
				return Err(anyhow!("Please specify the contract address."));
			},
		};
		let call_exec = match set_up_call(CallOpts {
			path: self.path.clone(),
			contract,
			message,
			args: self.args.clone(),
			value: self.value.clone(),
			gas_limit: self.gas_limit,
			proof_size: self.proof_size,
			url: self.url.clone(),
			suri: self.suri.clone(),
			execute: self.execute,
		})
		.await
		{
			Ok(call_exec) => call_exec,
			Err(e) => {
				return Err(anyhow!(format!("{}", e.to_string())));
			},
		};

		if self.dry_run {
			let spinner = spinner();
			spinner.start("Doing a dry run to estimate the gas...");
			match dry_run_gas_estimate_call(&call_exec).await {
				Ok(w) => {
					cli.info(format!("Gas limit: {:?}", w))?;
					cli.warning("Your call has not been executed.")?;
				},
				Err(e) => {
					spinner.error(format!("{e}"));
					display_message("Call failed.", false, cli)?;
				},
			};
			return Ok(());
		}

		if !self.execute {
			let spinner = spinner();
			spinner.start("Calling the contract...");
			let call_dry_run_result = dry_run_call(&call_exec).await?;
			cli.info(format!("Result: {}", call_dry_run_result))?;
			cli.warning("Your call has not been executed.")?;
		} else {
			let weight_limit = if self.gas_limit.is_some() && self.proof_size.is_some() {
				Weight::from_parts(self.gas_limit.unwrap(), self.proof_size.unwrap())
			} else {
				let spinner = spinner();
				spinner.start("Doing a dry run to estimate the gas...");
				match dry_run_gas_estimate_call(&call_exec).await {
					Ok(w) => {
						cli.info(format!("Gas limit: {:?}", w))?;
						w
					},
					Err(e) => {
						spinner.error(format!("{e}"));
						return Err(anyhow!("Call failed."));
					},
				}
			};
			let spinner = spinner();
			spinner.start("Calling the contract...");

			let call_result = call_smart_contract(call_exec, weight_limit, &self.url)
				.await
				.map_err(|err| anyhow!("{} {}", "ERROR:", format!("{err:?}")))?;

			cli.info(call_result)?;
		}

		// Prompt for any additional calls.
		if !prompt_to_repeat_call {
			display_message("Call completed successfully!", true, cli)?;
			return Ok(());
		}
		if cli
			.confirm("Do you want to perform another call using the existing smart contract?")
			.initial_value(false)
			.interact()?
		{
			// Reset specific items from the last call and repeat.
			self.reset_for_new_call();
			self.configure(cli, true).await?;
			Box::pin(self.execute_call(cli, prompt_to_repeat_call)).await
		} else {
			display_message("Contract calling complete.", true, cli)?;
			Ok(())
		}
	}

	/// Resets message specific fields to default values for a new call.
	fn reset_for_new_call(&mut self) {
		self.message = None;
		self.value = DEFAULT_PAYABLE_VALUE.to_string();
		self.gas_limit = None;
		self.proof_size = None;
	}
}

fn display_message(message: &str, success: bool, cli: &mut impl Cli) -> Result<()> {
	if success {
		cli.outro(message)?;
	} else {
		cli.outro_cancel(message)?;
	}
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::cli::MockCli;
	use pop_contracts::{mock_build_process, new_environment};
	use std::env;
	use url::Url;

	#[tokio::test]
	async fn execute_query_works() -> Result<()> {
		let temp_dir = new_environment("testing")?;
		let mut current_dir = env::current_dir().expect("Failed to get current directory");
		current_dir.pop();
		mock_build_process(
			temp_dir.path().join("testing"),
			current_dir.join("pop-contracts/tests/files/testing.contract"),
			current_dir.join("pop-contracts/tests/files/testing.json"),
		)?;
		// Contract deployed on Pop Network testnet, test get
		CallContractCommand {
			path: Some(temp_dir.path().join("testing")),
			contract: Some("15XausWjFLBBFLDXUSBRfSfZk25warm4wZRV4ZxhZbfvjrJm".to_string()),
			message: Some("get".to_string()),
			args: vec![].to_vec(),
			value: "0".to_string(),
			gas_limit: None,
			proof_size: None,
			url: Url::parse("wss://rpc1.paseo.popnetwork.xyz")?,
			suri: "//Alice".to_string(),
			dry_run: false,
			execute: false,
			dev_mode: false,
		}
		.execute()
		.await?;
		Ok(())
	}

	#[tokio::test]
	async fn call_contract_dry_run_works() -> Result<()> {
		let temp_dir = new_environment("testing")?;
		let mut current_dir = env::current_dir().expect("Failed to get current directory");
		current_dir.pop();
		mock_build_process(
			temp_dir.path().join("testing"),
			current_dir.join("pop-contracts/tests/files/testing.contract"),
			current_dir.join("pop-contracts/tests/files/testing.json"),
		)?;

		let mut cli = MockCli::new()
			.expect_intro(&"Call a contract")
			.expect_warning("Your call has not been executed.")
			.expect_info("Gas limit: Weight { ref_time: 100, proof_size: 10 }");

		let call_config = CallContractCommand {
			path: Some(temp_dir.path().join("testing")),
			contract: Some("15XausWjFLBBFLDXUSBRfSfZk25warm4wZRV4ZxhZbfvjrJm".to_string()),
			message: Some("flip".to_string()),
			args: vec![].to_vec(),
			value: "0".to_string(),
			gas_limit: Some(100),
			proof_size: Some(10),
			url: Url::parse("wss://rpc1.paseo.popnetwork.xyz")?,
			suri: "//Alice".to_string(),
			dry_run: true,
			execute: false,
			dev_mode: false,
		};
		call_config.configure(&mut cli, false).await?;
		assert_eq!(call_config.display(), format!(
			"pop call contract --path {} --contract 15XausWjFLBBFLDXUSBRfSfZk25warm4wZRV4ZxhZbfvjrJm --message flip --gas 100 --proof_size 10 --url wss://rpc1.paseo.popnetwork.xyz/ --suri //Alice --dry_run",
			temp_dir.path().join("testing").display().to_string(),
		));
		// Contract deployed on Pop Network testnet, test dry-run
		call_config.execute_call(&mut cli, false).await?;

		cli.verify()
	}

	#[tokio::test]
	async fn call_contract_query_duplicate_call_works() -> Result<()> {
		let temp_dir = new_environment("testing")?;
		let mut current_dir = env::current_dir().expect("Failed to get current directory");
		current_dir.pop();
		mock_build_process(
			temp_dir.path().join("testing"),
			current_dir.join("pop-contracts/tests/files/testing.contract"),
			current_dir.join("pop-contracts/tests/files/testing.json"),
		)?;
		let items = vec![
			("flip\n".into(), " A message that can be called on instantiated contracts.  This one flips the value of the stored `bool` from `true`  to `false` and vice versa.".into()),
			("get\n".into(), " Simply returns the current value of our `bool`.".into()),
			("specific_flip\n".into(), " A message for testing, flips the value of the stored `bool` with `new_value`  and is payable".into())
		];
		let mut cli = MockCli::new()
			.expect_intro(&"Call a contract")
			.expect_warning("Your call has not been executed.")
			.expect_confirm(
				"Do you want to perform another call using the existing smart contract?",
				false,
			)
			.expect_confirm(
				"Do you want to perform another call using the existing smart contract?",
				true,
			)
			.expect_select::<PathBuf>(
				"Select the message to call:",
				Some(false),
				true,
				Some(items),
				1, // "get" message
			)
			.expect_input("Signer calling the contract:", "//Alice".into())
			.expect_info(format!(
			    "pop call contract --path {} --contract 15XausWjFLBBFLDXUSBRfSfZk25warm4wZRV4ZxhZbfvjrJm --message get --url wss://rpc1.paseo.popnetwork.xyz/ --suri //Alice",
			    temp_dir.path().join("testing").display().to_string(),
			))
			.expect_warning("Your call has not been executed.")
			.expect_outro("Contract calling complete.");

		// Contract deployed on Pop Network testnet, test get
		let call_config = CallContractCommand {
			path: Some(temp_dir.path().join("testing")),
			contract: Some("15XausWjFLBBFLDXUSBRfSfZk25warm4wZRV4ZxhZbfvjrJm".to_string()),
			message: Some("get".to_string()),
			args: vec![].to_vec(),
			value: "0".to_string(),
			gas_limit: None,
			proof_size: None,
			url: Url::parse("wss://rpc1.paseo.popnetwork.xyz")?,
			suri: "//Alice".to_string(),
			dry_run: false,
			execute: false,
			dev_mode: false,
		};
		call_config.configure(&mut cli, false).await?;
		// Test the query. With true, it will prompt for another call.
		call_config.execute_call(&mut cli, true).await?;

		cli.verify()
	}

	// This test only covers the interactive portion of the call contract command, without actually
	// calling the contract.
	#[tokio::test]
	async fn guide_user_to_query_contract_works() -> Result<()> {
		let temp_dir = new_environment("testing")?;
		let mut current_dir = env::current_dir().expect("Failed to get current directory");
		current_dir.pop();
		mock_build_process(
			temp_dir.path().join("testing"),
			current_dir.join("pop-contracts/tests/files/testing.contract"),
			current_dir.join("pop-contracts/tests/files/testing.json"),
		)?;

		let items = vec![
			("flip\n".into(), " A message that can be called on instantiated contracts.  This one flips the value of the stored `bool` from `true`  to `false` and vice versa.".into()),
			("get\n".into(), " Simply returns the current value of our `bool`.".into()),
			("specific_flip\n".into(), " A message for testing, flips the value of the stored `bool` with `new_value`  and is payable".into())
		];
		// The inputs are processed in reverse order.
		let mut cli = MockCli::new()
			.expect_input("Signer calling the contract:", "//Alice".into())
			.expect_select::<PathBuf>(
				"Select the message to call:",
				Some(false),
				true,
				Some(items),
				1, // "get" message
			)
			.expect_input(
				"Paste the on-chain contract address:",
				"15XausWjFLBBFLDXUSBRfSfZk25warm4wZRV4ZxhZbfvjrJm".into(),
			)
			.expect_input(
				"Where is your contract deployed?",
				"wss://rpc1.paseo.popnetwork.xyz".into(),
			)
			.expect_input(
				"Where is your project located?",
				temp_dir.path().join("testing").display().to_string(),
			).expect_info(format!(
	            "pop call contract --path {} --contract 15XausWjFLBBFLDXUSBRfSfZk25warm4wZRV4ZxhZbfvjrJm --message get --url wss://rpc1.paseo.popnetwork.xyz/ --suri //Alice",
	            temp_dir.path().join("testing").display().to_string(),
	        ));

		let mut call_config = CallContractCommand {
			path: None,
			contract: None,
			message: None,
			args: vec![].to_vec(),
			value: DEFAULT_PAYABLE_VALUE.to_string(),
			gas_limit: None,
			proof_size: None,
			url: Url::parse(DEFAULT_URL)?,
			suri: DEFAULT_URI.to_string(),
			dry_run: false,
			execute: false,
			dev_mode: false,
		};
		call_config.configure(&mut cli, false).await?;
		assert_eq!(
			call_config.contract,
			Some("15XausWjFLBBFLDXUSBRfSfZk25warm4wZRV4ZxhZbfvjrJm".to_string())
		);
		assert_eq!(call_config.message, Some("get".to_string()));
		assert_eq!(call_config.args.len(), 0);
		assert_eq!(call_config.value, "0".to_string());
		assert_eq!(call_config.gas_limit, None);
		assert_eq!(call_config.proof_size, None);
		assert_eq!(call_config.url.to_string(), "wss://rpc1.paseo.popnetwork.xyz/");
		assert_eq!(call_config.suri, "//Alice");
		assert!(!call_config.execute);
		assert!(!call_config.dry_run);
		assert_eq!(call_config.display(), format!(
			"pop call contract --path {} --contract 15XausWjFLBBFLDXUSBRfSfZk25warm4wZRV4ZxhZbfvjrJm --message get --url wss://rpc1.paseo.popnetwork.xyz/ --suri //Alice",
			temp_dir.path().join("testing").display().to_string(),
		));

		cli.verify()
	}

	// This test only covers the interactive portion of the call contract command, without actually
	// calling the contract.
	#[tokio::test]
	async fn guide_user_to_call_contract_works() -> Result<()> {
		let temp_dir = new_environment("testing")?;
		let mut current_dir = env::current_dir().expect("Failed to get current directory");
		current_dir.pop();
		mock_build_process(
			temp_dir.path().join("testing"),
			current_dir.join("pop-contracts/tests/files/testing.contract"),
			current_dir.join("pop-contracts/tests/files/testing.json"),
		)?;

		let items = vec![
			("flip\n".into(), " A message that can be called on instantiated contracts.  This one flips the value of the stored `bool` from `true`  to `false` and vice versa.".into()),
			("get\n".into(), " Simply returns the current value of our `bool`.".into()),
			("specific_flip\n".into(), " A message for testing, flips the value of the stored `bool` with `new_value`  and is payable".into())
		];
		// The inputs are processed in reverse order.
		let mut cli = MockCli::new()
			.expect_confirm("Do you want to execute the call? (Selecting 'No' will perform a dry run)", true)
			.expect_input("Signer calling the contract:", "//Alice".into())
			.expect_input("Enter the proof size limit:", "".into()) // Only if call
			.expect_input("Enter the gas limit:", "".into()) // Only if call
			.expect_input("Value to transfer to the call:", "50".into()) // Only if payable
			.expect_input("Enter the value for the parameter: number", "2".into()) // Args for specific_flip
			.expect_input("Enter the value for the parameter: new_value", "true".into()) // Args for specific_flip
			.expect_select::<PathBuf>(
				"Select the message to call:",
				Some(false),
				true,
				Some(items),
				2, // "specific_flip" message
			)
			.expect_input(
				"Paste the on-chain contract address:",
				"15XausWjFLBBFLDXUSBRfSfZk25warm4wZRV4ZxhZbfvjrJm".into(),
			)
			.expect_input(
				"Where is your contract deployed?",
				"wss://rpc1.paseo.popnetwork.xyz".into(),
			)
			.expect_input(
				"Where is your project located?",
				temp_dir.path().join("testing").display().to_string(),
			).expect_info(format!(
				"pop call contract --path {} --contract 15XausWjFLBBFLDXUSBRfSfZk25warm4wZRV4ZxhZbfvjrJm --message specific_flip --args true,2 --value 50 --url wss://rpc1.paseo.popnetwork.xyz/ --suri //Alice --execute",
				temp_dir.path().join("testing").display().to_string(),
			));

		let mut call_config = CallContractCommand {
			path: None,
			contract: None,
			message: None,
			args: vec![].to_vec(),
			value: DEFAULT_PAYABLE_VALUE.to_string(),
			gas_limit: None,
			proof_size: None,
			url: Url::parse(DEFAULT_URL)?,
			suri: DEFAULT_URI.to_string(),
			dry_run: false,
			execute: false,
			dev_mode: false,
		};
		call_config.configure(&mut cli, false).await?;
		assert_eq!(
			call_config.contract,
			Some("15XausWjFLBBFLDXUSBRfSfZk25warm4wZRV4ZxhZbfvjrJm".to_string())
		);
		assert_eq!(call_config.message, Some("specific_flip".to_string()));
		assert_eq!(call_config.args.len(), 2);
		assert_eq!(call_config.args[0], "true".to_string());
		assert_eq!(call_config.args[1], "2".to_string());
		assert_eq!(call_config.value, "50".to_string());
		assert_eq!(call_config.gas_limit, None);
		assert_eq!(call_config.proof_size, None);
		assert_eq!(call_config.url.to_string(), "wss://rpc1.paseo.popnetwork.xyz/");
		assert_eq!(call_config.suri, "//Alice");
		assert!(call_config.execute);
		assert!(!call_config.dry_run);
		assert_eq!(call_config.display(), format!(
			"pop call contract --path {} --contract 15XausWjFLBBFLDXUSBRfSfZk25warm4wZRV4ZxhZbfvjrJm --message specific_flip --args true,2 --value 50 --url wss://rpc1.paseo.popnetwork.xyz/ --suri //Alice --execute",
			temp_dir.path().join("testing").display().to_string(),
		));

		cli.verify()
	}

	// This test only covers the interactive portion of the call contract command, without actually
	// calling the contract.
	#[tokio::test]
	async fn guide_user_to_call_contract_in_dev_mode_works() -> Result<()> {
		let temp_dir = new_environment("testing")?;
		let mut current_dir = env::current_dir().expect("Failed to get current directory");
		current_dir.pop();
		mock_build_process(
			temp_dir.path().join("testing"),
			current_dir.join("pop-contracts/tests/files/testing.contract"),
			current_dir.join("pop-contracts/tests/files/testing.json"),
		)?;

		let items = vec![
			("flip\n".into(), " A message that can be called on instantiated contracts.  This one flips the value of the stored `bool` from `true`  to `false` and vice versa.".into()),
			("get\n".into(), " Simply returns the current value of our `bool`.".into()),
			("specific_flip\n".into(), " A message for testing, flips the value of the stored `bool` with `new_value`  and is payable".into())
		];
		// The inputs are processed in reverse order.
		let mut cli = MockCli::new()
			.expect_input("Signer calling the contract:", "//Alice".into())
			.expect_input("Value to transfer to the call:", "50".into()) // Only if payable
			.expect_input("Enter the value for the parameter: new_value", "true".into()) // Args for specific_flip
			.expect_select::<PathBuf>(
				"Select the message to call:",
				Some(false),
				true,
				Some(items),
				2, // "specific_flip" message
			)
			.expect_input(
				"Paste the on-chain contract address:",
				"15XausWjFLBBFLDXUSBRfSfZk25warm4wZRV4ZxhZbfvjrJm".into(),
			)
			.expect_input(
				"Where is your contract deployed?",
				"wss://rpc1.paseo.popnetwork.xyz".into(),
			)
			.expect_input(
				"Where is your project located?",
				temp_dir.path().join("testing").display().to_string(),
			).expect_info(format!(
				"pop call contract --path {} --contract 15XausWjFLBBFLDXUSBRfSfZk25warm4wZRV4ZxhZbfvjrJm --message specific_flip --args true --value 50 --url wss://rpc1.paseo.popnetwork.xyz/ --suri //Alice --execute",
				temp_dir.path().join("testing").display().to_string(),
			));

		let mut call_config = CallContractCommand {
			path: None,
			contract: None,
			message: None,
			args: vec![].to_vec(),
			value: DEFAULT_PAYABLE_VALUE.to_string(),
			gas_limit: None,
			proof_size: None,
			url: Url::parse(DEFAULT_URL)?,
			suri: DEFAULT_URI.to_string(),
			dry_run: false,
			execute: false,
			dev_mode: true,
		};
		call_config.configure(&mut cli, false).await?;
		assert_eq!(
			call_config.contract,
			Some("15XausWjFLBBFLDXUSBRfSfZk25warm4wZRV4ZxhZbfvjrJm".to_string())
		);
		assert_eq!(call_config.message, Some("specific_flip".to_string()));
		assert_eq!(call_config.args.len(), 1);
		assert_eq!(call_config.args[0], "true".to_string());
		assert_eq!(call_config.value, "50".to_string());
		assert_eq!(call_config.gas_limit, None);
		assert_eq!(call_config.proof_size, None);
		assert_eq!(call_config.url.to_string(), "wss://rpc1.paseo.popnetwork.xyz/");
		assert_eq!(call_config.suri, "//Alice");
		assert!(call_config.execute);
		assert!(!call_config.dry_run);
		assert!(call_config.dev_mode);
		assert_eq!(call_config.display(), format!(
			"pop call contract --path {} --contract 15XausWjFLBBFLDXUSBRfSfZk25warm4wZRV4ZxhZbfvjrJm --message specific_flip --args true --value 50 --url wss://rpc1.paseo.popnetwork.xyz/ --suri //Alice --execute",
			temp_dir.path().join("testing").display().to_string(),
		));

		cli.verify()
	}

	#[tokio::test]
	async fn guide_user_to_call_contract_fails_not_build() -> Result<()> {
		let temp_dir = new_environment("testing")?;
		let mut cli = MockCli::new();
		assert!(matches!(CallContractCommand {
			path: Some(temp_dir.path().join("testing")),
			contract: None,
			message: None,
			args: vec![].to_vec(),
			value: "0".to_string(),
			gas_limit: None,
			proof_size: None,
			url: Url::parse("wss://rpc1.paseo.popnetwork.xyz")?,
			suri: "//Alice".to_string(),
			dry_run: false,
			execute: false,
			dev_mode: false,
		}.configure(&mut cli, false).await, Err(message) if message.to_string().contains("Unable to fetch contract metadata: Failed to find any contract artifacts in target directory.")));
		cli.verify()
	}

	#[tokio::test]
	async fn execute_contract_fails_no_message_or_contract() -> Result<()> {
		let temp_dir = new_environment("testing")?;
		let mut current_dir = env::current_dir().expect("Failed to get current directory");
		current_dir.pop();
		mock_build_process(
			temp_dir.path().join("testing"),
			current_dir.join("pop-contracts/tests/files/testing.contract"),
			current_dir.join("pop-contracts/tests/files/testing.json"),
		)?;

		let mut cli = MockCli::new();
		assert!(matches!(
			CallContractCommand {
				path: Some(temp_dir.path().join("testing")),
				contract: Some("15XausWjFLBBFLDXUSBRfSfZk25warm4wZRV4ZxhZbfvjrJm".to_string()),
				message: None,
				args: vec![].to_vec(),
				value: "0".to_string(),
				gas_limit: None,
				proof_size: None,
				url: Url::parse("wss://rpc1.paseo.popnetwork.xyz")?,
				suri: "//Alice".to_string(),
				dry_run: false,
				execute: false,
				dev_mode: false,
			}.execute_call(&mut cli, false).await,
			anyhow::Result::Err(message) if message.to_string() == "Please specify the message to call."
		));

		assert!(matches!(
			CallContractCommand {
				path: Some(temp_dir.path().join("testing")),
				contract: None,
				message: Some("get".to_string()),
				args: vec![].to_vec(),
				value: "0".to_string(),
				gas_limit: None,
				proof_size: None,
				url: Url::parse("wss://rpc1.paseo.popnetwork.xyz")?,
				suri: "//Alice".to_string(),
				dry_run: false,
				execute: false,
				dev_mode: false,
			}.execute_call(&mut cli, false).await,
			anyhow::Result::Err(message) if message.to_string() == "Please specify the contract address."
		));

		cli.verify()
	}

	#[test]
	fn display_message_works() -> Result<()> {
		let mut cli = MockCli::new().expect_outro(&"Call completed successfully!");
		display_message("Call completed successfully!", true, &mut cli)?;
		cli.verify()?;
		let mut cli = MockCli::new().expect_outro_cancel("Call failed.");
		display_message("Call failed.", false, &mut cli)?;
		cli.verify()
	}
}
