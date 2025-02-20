use anyhow::Result;
use clap::Parser;
use csv::Reader;
use frame_benchmarking_cli::PalletCmd;
use rust_fuzzy_search::fuzzy_search_best_n;
use sc_chain_spec::GenesisConfigBuilderRuntimeCaller;
use sp_runtime::traits::BlakeTwo256;
use std::{
	collections::HashMap,
	fs,
	fs::File,
	io::BufReader,
	path::{Path, PathBuf},
};
use stdio_override::StdoutOverride;
use tempfile::tempdir;

type HostFunctions = (
	sp_statement_store::runtime_api::HostFunctions,
	cumulus_primitives_proof_size_hostfunction::storage_proof_size::HostFunctions,
);

/// Type alias for records where the key is the pallet name and the value is a array of its
/// extrinsics.
pub type PalletExtrinsicsCollection = HashMap<String, Vec<String>>;

/// Check if a runtime has a genesis config preset.
///
/// # Arguments
/// * `binary_path` - Path to the runtime WASM binary.
/// * `preset` - Optional ID of the genesis config preset. If not provided, it checks the default
///   preset.
pub fn check_preset(binary_path: &PathBuf, preset: Option<&String>) -> anyhow::Result<()> {
	let binary = fs::read(binary_path).map_err(anyhow::Error::from)?;
	let genesis_config_builder = GenesisConfigBuilderRuntimeCaller::<HostFunctions>::new(&binary);
	if genesis_config_builder.get_named_preset(preset).is_err() {
		return Err(anyhow::anyhow!(format!(
			r#"The preset with name "{:?}" is not available."#,
			preset
		)))
	}
	Ok(())
}

/// Get the runtime folder path and throws error if not exist.
///
/// # Arguments
/// * `parent` - Parent path that contains the runtime folder.
pub fn get_runtime_path(parent: &Path) -> anyhow::Result<PathBuf> {
	["runtime", "runtimes"]
		.iter()
		.map(|f| parent.join(f))
		.find(|path| path.exists())
		.ok_or_else(|| anyhow::anyhow!("No runtime found."))
}

/// List a mapping of pallets and their extrinsics.
///
/// # Arguments
/// * `runtime_path` - Path to the runtime WASM binary.
pub fn list_pallets_and_extrinsics(
	runtime_path: &Path,
) -> anyhow::Result<PalletExtrinsicsCollection> {
	let temp_dir = tempdir()?;
	let temp_file_path = temp_dir.path().join("pallets.csv");
	let guard = StdoutOverride::from_file(&temp_file_path)?;
	let cmd = PalletCmd::try_parse_from([
		"",
		"--runtime",
		runtime_path.to_str().unwrap(),
		"--genesis-builder",
		"none", // For parsing purpose.
		"--list=all",
	])?;
	cmd.run_with_spec::<BlakeTwo256, HostFunctions>(None)
		.map_err(|e| anyhow::anyhow!(format!("Failed to list pallets: {}", e.to_string())))?;
	drop(guard);
	parse_csv_to_map(&temp_file_path)
}

/// Parse the pallet command from string value of genesis policy builder.
///
/// # Arguments
/// * `policy` - Genesis builder policy ( none | spec | runtime ).
pub fn parse_genesis_builder_policy(policy: &str) -> anyhow::Result<PalletCmd> {
	PalletCmd::try_parse_from([
		"",
		"--list",
		"--runtime",
		"dummy-runtime", // For parsing purpose.
		"--genesis-builder",
		policy,
	])
	.map_err(|e| {
		anyhow::anyhow!(format!(r#"Invalid genesis builder option {policy}: {}"#, e.to_string()))
	})
}

fn parse_csv_to_map(file_path: &PathBuf) -> anyhow::Result<PalletExtrinsicsCollection> {
	let file = File::open(file_path)?;
	let mut rdr = Reader::from_reader(BufReader::new(file));
	let mut map: PalletExtrinsicsCollection = HashMap::new();
	for result in rdr.records() {
		let record = result?;
		if record.len() == 2 {
			let pallet = record[0].trim().to_string();
			let extrinsic = record[1].trim().to_string();
			map.entry(pallet).or_default().push(extrinsic);
		}
	}
	Ok(map)
}

/// Run command for pallet benchmarking.
///
/// # Arguments
/// * `cmd` - Command to benchmark the FRAME Pallets.
pub fn run_pallet_benchmarking(cmd: &PalletCmd) -> Result<()> {
	cmd.run_with_spec::<BlakeTwo256, HostFunctions>(None)
		.map_err(|e| anyhow::anyhow!(format!("Failed to run benchmarking: {}", e.to_string())))
}

/// Performs a fuzzy search for pallets that match the provided input.
///
/// # Arguments
/// * `pallet_extrinsics` - A mapping of pallets and their extrinsics.
/// * `input` - The search input used to match pallets.
pub fn search_for_pallets(
	pallet_extrinsics: &PalletExtrinsicsCollection,
	input: &str,
	limit: usize,
) -> Vec<String> {
	let pallets = pallet_extrinsics.keys();

	if input.is_empty() {
		return pallets.map(String::from).take(limit).collect();
	}
	let inputs = input.split(",");
	let pallets: Vec<&str> = pallets.map(|s| s.as_str()).collect();
	let mut output = inputs
		.flat_map(|input| fuzzy_search_best_n(input, &pallets, limit))
		.map(|v| v.0.to_string())
		.collect::<Vec<String>>();
	output.dedup();
	output
}

/// Performs a fuzzy search for extrinsics that match the provided input.
///
/// # Arguments
/// * `pallet_extrinsics` - A mapping of pallets and their extrinsics.
/// * `pallets` - List of pallets used to find the extrinsics.
/// * `input` - The search input used to match extrinsics.
pub fn search_for_extrinsics(
	pallet_extrinsics: &PalletExtrinsicsCollection,
	pallets: Vec<String>,
	input: &str,
	limit: usize,
) -> Vec<String> {
	let extrinsics: Vec<&str> = pallet_extrinsics
		.iter()
		.filter(|(pallet, _)| pallets.contains(pallet))
		.flat_map(|(_, extrinsics)| extrinsics.iter().map(String::as_str))
		.collect();

	if input.is_empty() {
		return extrinsics.into_iter().map(String::from).take(limit).collect();
	}
	let inputs = input.split(",");
	let mut output = inputs
		.flat_map(|input| fuzzy_search_best_n(input, &extrinsics, limit))
		.map(|v| v.0.to_string())
		.collect::<Vec<String>>();
	output.dedup();
	output
}

#[cfg(test)]
mod tests {
	use super::*;
	use tempfile::tempdir;

	#[test]
	fn check_preset_works() -> anyhow::Result<()> {
		let runtime_path = std::env::current_dir()
			.unwrap()
			.join("../../tests/runtimes/base_parachain_benchmark.wasm")
			.canonicalize()?;
		assert!(check_preset(&runtime_path, Some(&"development".to_string())).is_ok());
		assert!(check_preset(&runtime_path, Some(&"random-preset-name".to_string())).is_err());
		Ok(())
	}

	#[test]
	fn get_runtime_path_works() -> anyhow::Result<()> {
		let temp_dir = tempdir()?;
		for name in ["runtime", "runtimes"] {
			let path = temp_dir.path();
			fs::create_dir(&path.join(name))?;
			get_runtime_path(&path)?;
		}
		Ok(())
	}

	#[test]
	fn list_pallets_and_extrinsics_works() -> Result<()> {
		let runtime_path = std::env::current_dir()
			.unwrap()
			.join("../../tests/runtimes/base_parachain_benchmark.wasm")
			.canonicalize()
			.unwrap();

		let pallets = list_pallets_and_extrinsics(&runtime_path)?;
		assert_eq!(
			pallets.get("pallet_timestamp").cloned().unwrap_or_default(),
			["on_finalize", "set"]
		);
		assert_eq!(
			pallets.get("pallet_sudo").cloned().unwrap_or_default(),
			["check_only_sudo_account", "remove_key", "set_key", "sudo", "sudo_as"]
		);
		Ok(())
	}

	#[test]
	fn parse_genesis_builder_policy_works() -> anyhow::Result<()> {
		for policy in ["none", "runtime"] {
			parse_genesis_builder_policy(policy)?;
		}
		Ok(())
	}
}
