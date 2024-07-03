use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Serialize, Debug)]
pub struct JoinRequest {
	pub reason: String,
	pub third_party: ThirdPartySigned,
}

#[derive(Serialize, Debug)]
pub struct ThirdPartySigned {
	pub mxid: String,
	pub sender: String,
	pub signatures: Map<String, Value>,
	pub token: String,
}

#[derive(Deserialize, Debug)]
pub struct JoinResponse {
	pub room_id: String,
}
