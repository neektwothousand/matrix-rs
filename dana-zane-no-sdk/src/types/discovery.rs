use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct DiscoveryInfo {
	#[serde(rename = "m.homeserver")]
	pub m_homeserver: HomeserverInfo,

	#[serde(rename = "m.identity_server", skip_serializing_if = "Option::is_none")]
	pub m_identity_server: Option<IdentityServerInfo>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Versions {
	pub versions: Vec<String>,
	pub unstable_features: serde_json::Value,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct HomeserverInfo {
	pub base_url: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct IdentityServerInfo {
	pub base_url: String,
}
