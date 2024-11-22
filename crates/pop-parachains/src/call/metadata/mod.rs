// SPDX-License-Identifier: GPL-3.0

use crate::errors::Error;
use scale_info::{form::PortableForm, Variant};
use subxt::{dynamic::Value, Metadata, OnlineClient, SubstrateConfig};
use type_parser::process_argument;

pub mod action;
mod params;
mod type_parser;

#[derive(Clone, PartialEq, Eq)]
/// Represents a pallet in the blockchain, including its extrinsics.
pub struct Pallet {
	/// The name of the pallet.
	pub name: String,
	/// The documentation of the pallet.
	pub docs: String,
	// The extrinsics of the pallet.
	pub extrinsics: Vec<Extrinsic>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
/// Represents an extrinsic in a pallet.
pub struct Extrinsic {
	/// The name of the extrinsic.
	pub name: String,
	/// The documentation of the extrinsic.
	pub docs: String,
	/// The parameters of the extrinsic.
	pub params: Vec<Param>,
	/// Whether this extrinsic is supported (no recursive or unsupported types like `RuntimeCall`).
	pub is_supported: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Describes a parameter of an extrinsic.
pub struct Param {
	/// The name of the parameter.
	pub name: String,
	/// The type of the parameter.
	pub type_name: String,
	/// Indicates if the parameter is optional (`Option<T>`).
	pub is_optional: bool,
	/// Nested parameters for composite or variants types.
	pub sub_params: Vec<Param>,
	/// Indicates if the parameter is a Variant.
	pub is_variant: bool,
}

/// Parses the chain metadata to extract information about pallets and their extrinsics with its
/// parameters.
///
/// # Arguments
/// * `api`: Reference to an `OnlineClient` connected to the chain.
pub async fn parse_chain_metadata(
	api: &OnlineClient<SubstrateConfig>,
) -> Result<Vec<Pallet>, Error> {
	let metadata: Metadata = api.metadata();

	let pallets = metadata
		.pallets()
		.map(|pallet| {
			let extrinsics = pallet
				.call_variants()
				.map(|variants| {
					variants
						.iter()
						.map(|variant| {
							let mut is_supported = true;

							// Parse parameters for the extrinsic
							let params = {
								let mut parsed_params = Vec::new();
								for field in &variant.fields {
									match params::field_to_param(api, &variant.name, field) {
										Ok(param) => parsed_params.push(param),
										Err(Error::ExtrinsicNotSupported(_)) => {
											// Unsupported extrinsic due to complex types
											is_supported = false;
											parsed_params.clear();
											break;
										},
										Err(e) => return Err(e),
									}
								}
								parsed_params
							};

							Ok(Extrinsic {
								name: variant.name.clone(),
								docs: if is_supported {
									variant.docs.concat()
								} else {
									// To display the message in the UI
									"Extrinsic Not Supported".to_string()
								},
								params,
								is_supported,
							})
						})
						.collect::<Result<Vec<Extrinsic>, Error>>()
				})
				.unwrap_or_else(|| Ok(vec![]))?;

			Ok(Pallet {
				name: pallet.name().to_string(),
				docs: pallet.docs().join(" "),
				extrinsics,
			})
		})
		.collect::<Result<Vec<Pallet>, Error>>()?;

	Ok(pallets)
}

/// Finds a specific pallet by name and retrieves its details from metadata.
///
/// # Arguments
/// * `pallets`: List of pallets availables in the chain.
/// * `pallet_name`: The name of the pallet to find.
pub async fn find_pallet_by_name(pallets: &[Pallet], pallet_name: &str) -> Result<Pallet, Error> {
	if let Some(pallet) = pallets.iter().find(|p| p.name == pallet_name) {
		Ok(pallet.clone())
	} else {
		Err(Error::PalletNotFound(pallet_name.to_string()))
	}
}

/// Finds a specific extrinsic by name and retrieves its details from metadata.
///
/// # Arguments
/// * `pallets`: List of pallets availables in the chain.
/// * `pallet_name`: The name of the pallet to find.
/// * `extrinsic_name`: Name of the extrinsic to locate.
pub async fn find_extrinsic_by_name(
	pallets: &[Pallet],
	pallet_name: &str,
	extrinsic_name: &str,
) -> Result<Extrinsic, Error> {
	let pallet = find_pallet_by_name(pallets, pallet_name).await?;
	if let Some(extrinsic) = pallet.extrinsics.iter().find(|&e| e.name == extrinsic_name) {
		return Ok(extrinsic.clone());
	} else {
		return Err(Error::ExtrinsicNotSupported(extrinsic_name.to_string()));
	}
}

/// Processes and maps parameters for a given pallet extrinsic based on its metadata.
///
/// # Arguments
/// * `api`: Reference to an `OnlineClient` connected to the blockchain.
/// * `pallet_name`: Name of the pallet containing the extrinsic.
/// * `extrinsic_name`: Name of the extrinsic to process.
/// * `raw_params`: A vector of raw string arguments for the extrinsic.
pub async fn process_extrinsic_args(
	api: &OnlineClient<SubstrateConfig>,
	pallet_name: &str,
	extrinsic_name: &str,
	raw_params: Vec<String>,
) -> Result<Vec<Value>, Error> {
	let metadata: Metadata = api.metadata();
	let registry = metadata.types();
	let extrinsic = parse_extrinsic_by_name(&api, pallet_name, extrinsic_name).await?;
	let mut processed_parameters: Vec<Value> = Vec::new();
	for (index, field) in extrinsic.fields.iter().enumerate() {
		let raw_parameter = raw_params.get(index).ok_or(Error::ParamProcessingError)?;
		let type_info = registry.resolve(field.ty.id).ok_or(Error::ParamProcessingError)?; //Resolve with type_id
		let arg_processed = process_argument(raw_parameter, type_info, registry)?;
		processed_parameters.push(arg_processed);
	}
	Ok(processed_parameters)
}

/// Finds a specific extrinsic by name and retrieves its details from metadata.
///
/// # Arguments
/// * `api`: Reference to an `OnlineClient` connected to the chain.
/// * `pallet_name`: The name of the pallet to find.
/// * `extrinsic_name`: Name of the extrinsic to locate.
async fn parse_extrinsic_by_name(
	api: &OnlineClient<SubstrateConfig>,
	pallet_name: &str,
	extrinsic_name: &str,
) -> Result<Variant<PortableForm>, Error> {
	let metadata: Metadata = api.metadata();
	let pallet = metadata
		.pallets()
		.into_iter()
		.find(|p| p.name() == pallet_name)
		.ok_or_else(|| Error::PalletNotFound(pallet_name.to_string()))?;
	// Retrieve and check for the extrinsic within the pallet
	let extrinsic = pallet
		.call_variants()
		.map(|variants| variants.iter().find(|e| e.name == extrinsic_name))
		.flatten()
		.ok_or_else(|| Error::ExtrinsicNotSupported(extrinsic_name.to_string()))?;

	Ok(extrinsic.clone())
}

#[cfg(test)]
mod tests {
	use crate::set_up_api;

	use super::*;
	use anyhow::Result;

	// #[tokio::test]
	// async fn process_prompt_arguments_works() -> Result<()> {
	// 	let api = set_up_api("ws://127.0.0.1:9944").await?;
	// 	// let ex = find_extrinsic_by_name(&api, "Balances", "transfer_allow_death").await?;
	// 	let ex = find_extrinsic_by_name(&api, "Nfts", "mint").await?;
	// 	let prompt_args1 = process_prompt_arguments(&api, &ex.fields()[2])?;

	// 	Ok(())
	// }

	// #[tokio::test]
	// async fn process_extrinsic_args_works() -> Result<()> {
	// 	let api = set_up_api("ws://127.0.0.1:9944").await?;
	// 	// let ex = find_extrinsic_by_name(&api, "Balances", "transfer_allow_death").await?;
	// 	let ex = parse_extrinsic_by_name(&api, "Utility", "batch").await?;
	// 	println!("EXTRINSIC {:?}", ex);
	// 	println!(" ARGS PARSER {:?}", ex.fields);

	// 	Ok(())
	// }

	// #[tokio::test]
	// async fn process_extrinsic_args2_works() -> Result<()> {
	// 	let api = set_up_api("ws://127.0.0.1:9944").await?;
	// 	// let ex = find_extrinsic_by_name(&api, "Balances", "transfer_allow_death").await?;
	// 	let ex = find_extrinsic_by_name(&api, "Nfts", "mint").await?;
	// 	let args_parsed =
	// 		process_extrinsic_args(&api, "System", "remark", vec!["0x11".to_string()]).await?;
	// 	println!(" ARGS PARSER {:?}", args_parsed);

	// 	Ok(())
	// }
}
