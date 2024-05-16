// SPDX-License-Identifier: GPL-3.0
use strum::{
	EnumMessage as EnumMessageT, EnumProperty as EnumPropertyT, VariantArray as VariantArrayT,
};
use strum_macros::{AsRefStr, Display, EnumMessage, EnumProperty, EnumString, VariantArray};
use thiserror::Error;

#[derive(
	AsRefStr, Clone, Default, Debug, Display, EnumMessage, EnumString, Eq, PartialEq, VariantArray,
)]
pub enum Provider {
	#[default]
	#[strum(
		ascii_case_insensitive,
		serialize = "pop",
		message = "Pop",
		detailed_message = "An all-in-one tool for Polkadot development."
	)]
	Pop,
	#[strum(
		ascii_case_insensitive,
		serialize = "openzeppelin",
		message = "OpenZeppelin",
		detailed_message = "The standard for secure blockchain applications."
	)]
	OpenZeppelin,
	#[strum(
		ascii_case_insensitive,
		serialize = "parity",
		message = "Parity",
		detailed_message = "Solutions for a trust-free world."
	)]
	Parity,
}

impl Provider {
	pub fn providers() -> &'static [Provider] {
		Provider::VARIANTS
	}

	pub fn name(&self) -> &str {
		self.get_message().unwrap_or_default()
	}

	pub fn default_template(&self) -> Template {
		match &self {
			Provider::Pop => Template::Standard,
			Provider::OpenZeppelin => Template::OpenZeppelinGeneric,
			Provider::Parity => Template::ParityContracts,
		}
	}

	pub fn description(&self) -> &str {
		self.get_detailed_message().unwrap_or_default()
	}

	pub fn templates(&self) -> Vec<&Template> {
		Template::VARIANTS
			.iter()
			.filter(|t| t.get_str("Provider") == Some(self.name()))
			.collect()
	}
}

#[derive(Debug, Clone, PartialEq)]
pub struct Config {
	pub symbol: String,
	pub decimals: u8,
	pub initial_endowment: String,
}

#[derive(
	AsRefStr,
	Clone,
	Debug,
	Default,
	Display,
	EnumMessage,
	EnumProperty,
	EnumString,
	Eq,
	Hash,
	PartialEq,
	VariantArray,
)]
pub enum Template {
	// Pop
	#[default]
	#[strum(
		serialize = "standard",
		message = "Standard",
		detailed_message = "A standard parachain",
		props(
			Provider = "Pop",
			Repository = "https://github.com/r0gue-io/base-parachain",
			Network = "./network.toml"
		)
	)]
	Standard,
	#[strum(
		serialize = "assets",
		message = "Assets",
		detailed_message = "Parachain configured with fungible and non-fungilble asset functionalities.",
		props(
			Provider = "Pop",
			Repository = "https://github.com/r0gue-io/assets-parachain",
			Network = "./network.toml"
		)
	)]
	Assets,
	#[strum(
		serialize = "contracts",
		message = "Contracts",
		detailed_message = "Parachain configured to support WebAssembly smart contracts.",
		props(
			Provider = "Pop",
			Repository = "https://github.com/r0gue-io/contracts-parachain",
			Network = "./network.toml"
		)
	)]
	Contracts,
	#[strum(
		serialize = "evm",
		message = "EVM",
		detailed_message = "Parachain configured with Frontier, enabling compatibility with the Ethereum Virtual Machine (EVM).",
		props(
			Provider = "Pop",
			Repository = "https://github.com/r0gue-io/evm-parachain",
			Network = "./network.toml"
		)
	)]
	EVM,
	// OpenZeppelin
	#[strum(
		serialize = "polkadot-generic-runtime-template",
		message = "Generic Runtime Template",
		detailed_message = "A generic template for Substrate Runtime",
		props(
			Provider = "OpenZeppelin",
			Repository = "https://github.com/OpenZeppelin/polkadot-generic-runtime-template",
			Network = "./zombienet-config/devnet.toml"
		)
	)]
	OpenZeppelinGeneric,
	#[strum(
		serialize = "polkadot-evm-runtime-template",
		message = "EVM Runtime Template",
		detailed_message = "EVM runtime template for Polkadot parachains ",
		props(
			Provider = "OpenZeppelin",
			Repository = "https://github.com/OpenZeppelin/polkadot-evm-runtime-template",
			Network = "./zombienet-config/devnet.toml"
		)
	)]
	OpenZeppelinEVM,
	// Parity
	#[strum(
		serialize = "cpt",
		message = "Contracts",
		detailed_message = "Minimal Substrate node configured for smart contracts via pallet-contracts.",
		props(
			Provider = "Parity",
			Repository = "https://github.com/paritytech/substrate-contracts-node",
			Network = "./zombienet.toml"
		)
	)]
	ParityContracts,
	#[strum(
		serialize = "fpt",
		message = "EVM",
		detailed_message = "Template node for a Frontier (EVM) based parachain.",
		props(
			Provider = "Parity",
			Repository = "https://github.com/paritytech/frontier-parachain-template",
			Network = "./zombienet-config.toml"
		)
	)]
	ParityFPT,
}

impl Template {
	pub fn name(&self) -> &str {
		self.get_message().unwrap_or_default()
	}
	pub fn description(&self) -> &str {
		self.get_detailed_message().unwrap_or_default()
	}

	pub fn matches(&self, provider: &Provider) -> bool {
		// Match explicitly on provider name (message)
		self.get_str("Provider") == Some(provider.name())
	}

	pub fn repository_url(&self) -> Result<&str, Error> {
		self.get_str("Repository").ok_or(Error::RepositoryMissing)
	}

	pub fn provider(&self) -> Result<&str, Error> {
		self.get_str("Provider").ok_or(Error::ProviderMissing)
	}

	/// Returns the relative path to the default network configuration file to be used, if defined.
	pub fn network_config(&self) -> Option<&str> {
		self.get_str("Network")
	}
}

#[derive(Error, Debug)]
pub enum Error {
	#[error("The `Repository` property is missing from the template variant")]
	RepositoryMissing,
	#[error("The `Provider` property is missing from the template variant")]
	ProviderMissing,
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::{collections::HashMap, str::FromStr};

	fn templates_names() -> HashMap<String, Template> {
		HashMap::from([
			("standard".to_string(), Template::Standard),
			("assets".to_string(), Template::Assets),
			("contracts".to_string(), Template::Contracts),
			("evm".to_string(), Template::EVM),
			("cpt".to_string(), Template::ParityContracts),
			("fpt".to_string(), Template::ParityFPT),
		])
	}

	fn templates_urls() -> HashMap<String, &'static str> {
		HashMap::from([
			("standard".to_string(), "https://github.com/r0gue-io/base-parachain"),
			("assets".to_string(), "https://github.com/r0gue-io/assets-parachain"),
			("contracts".to_string(), "https://github.com/r0gue-io/contracts-parachain"),
			("evm".to_string(), "https://github.com/r0gue-io/evm-parachain"),
			("cpt".to_string(), "https://github.com/paritytech/substrate-contracts-node"),
			("fpt".to_string(), "https://github.com/paritytech/frontier-parachain-template"),
		])
	}

	fn template_network_configs() -> HashMap<Template, Option<&'static str>> {
		[
			(Template::Standard, Some("./network.toml")),
			(Template::Assets, Some("./network.toml")),
			(Template::Contracts, Some("./network.toml")),
			(Template::EVM, Some("./network.toml")),
			(Template::OpenZeppelinGeneric, Some("./zombienet-config/devnet.toml")),
			(Template::OpenZeppelinEVM, Some("./zombienet-config/devnet.toml")),
			(Template::ParityContracts, Some("./zombienet.toml")),
			(Template::ParityFPT, Some("./zombienet-config.toml")),
		]
		.into()
	}

	#[test]
	fn test_is_template_correct() {
		for template in Template::VARIANTS {
			if matches!(
				template,
				Template::Standard | Template::Assets | Template::Contracts | Template::EVM
			) {
				assert_eq!(template.matches(&Provider::Pop), true);
				assert_eq!(template.matches(&Provider::Parity), false);
			}
			if matches!(template, Template::ParityContracts | Template::ParityFPT) {
				assert_eq!(template.matches(&Provider::Pop), false);
				assert_eq!(template.matches(&Provider::Parity), true);
			}
		}
	}

	#[test]
	fn test_convert_string_to_template() {
		let template_names = templates_names();
		// Test the default
		assert_eq!(Template::from_str("").unwrap_or_default(), Template::Standard);
		// Test the rest
		for template in Template::VARIANTS {
			assert_eq!(
				&Template::from_str(&template.to_string()).unwrap(),
				template_names.get(&template.to_string()).unwrap()
			);
		}
	}

	#[test]
	fn test_repository_url() {
		let template_urls = templates_urls();
		for template in Template::VARIANTS {
			assert_eq!(
				&template.repository_url().unwrap(),
				template_urls.get(&template.to_string()).unwrap()
			);
		}
	}

	#[test]
	fn test_network_config() {
		let network_configs = template_network_configs();
		for template in Template::VARIANTS {
			assert_eq!(template.network_config(), network_configs[template]);
		}
	}

	#[test]
	fn test_default_template_of_provider() {
		let mut provider = Provider::Pop;
		assert_eq!(provider.default_template(), Template::Standard);
		provider = Provider::Parity;
		assert_eq!(provider.default_template(), Template::ParityContracts);
	}

	#[test]
	fn test_templates_of_provider() {
		let mut provider = Provider::Pop;
		assert_eq!(
			provider.templates(),
			[&Template::Standard, &Template::Assets, &Template::Contracts, &Template::EVM]
		);
		provider = Provider::Parity;
		assert_eq!(provider.templates(), [&Template::ParityContracts, &Template::ParityFPT]);
	}

	#[test]
	fn test_convert_string_to_provider() {
		assert_eq!(Provider::from_str("Pop").unwrap(), Provider::Pop);
		assert_eq!(Provider::from_str("").unwrap_or_default(), Provider::Pop);
		assert_eq!(Provider::from_str("Parity").unwrap(), Provider::Parity);
	}
}
