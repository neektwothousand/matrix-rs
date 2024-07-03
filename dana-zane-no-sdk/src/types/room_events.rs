use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Debug)]
pub struct RoomEncryption {
	pub algorithm: String,
	pub rotation_period_ms: Option<u64>,
	pub rotation_period_msgs: Option<u64>,
}

#[derive(Serialize, Debug)]
pub struct SendRequest {
	pub body: String,
	pub msgtype: String,
}

#[derive(Serialize, Debug)]
pub struct SendEventRequest {
	pub body: String,
	pub msgtype: String,
}

#[derive(Serialize, Debug)]
pub struct SendToDeviceRequest {
	pub messages: HashMap<String, HashMap<String, ToDeviceEvent>>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ToDeviceEvent {
	pub content: RoomKeyRequest,
	pub r#type: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct RoomKeyRequest {
	pub action: String,
	pub body: Option<RequestKeyInfo>,
	pub request_id: String,
	pub requesting_device_id: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct RequestKeyInfo {
	pub algorithm: String,
	pub room_id: String,
	pub sender_key: Option<String>,
	pub session_id: String,
}

#[derive(Deserialize, Debug)]
pub struct EventResponse {
	pub event_id: String,
}

#[derive(Deserialize, Debug)]
pub struct SendResponse {
	pub event_id: String,
}
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RoomKey {
	pub algorithm: String,
	pub room_id: String,
	pub session_id: String,
	pub session_key: String,
}
