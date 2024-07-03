use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct LoginRequest {
	#[serde(skip_serializing_if = "Option::is_none")]
	pub device_id: Option<String>,
	pub identifier: Identifier,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub initial_device_display_name: Option<String>,
	pub password: String,
	pub r#type: String,
}
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Identifier {
	pub r#type: String,
	pub user: String,
}

#[derive(Debug, Deserialize)]
pub struct LoginResponse {
	pub user_id: String,
	pub access_token: String,
	pub home_server: String,
	pub device_id: String,
}
