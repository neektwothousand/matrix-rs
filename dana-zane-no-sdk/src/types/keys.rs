use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Serialize, Debug)]
pub struct KeysUploadRequest {
	pub device_keys: DeviceKeys,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub fallback_keys: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub one_time_keys: Option<Value>,
}

#[derive(Serialize, Debug)]
pub struct DeviceKeys {
	pub user_id: String,
	pub device_id: String,
	pub algorithms: Vec<String>,
	pub keys: Value,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub signatures: Option<Value>,
}

#[derive(Deserialize, Debug)]
pub struct KeysUploadResponse {
	pub one_time_key_counts: Value,
}
