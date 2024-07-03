#![allow(unused)]
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashMap;

#[derive(Serialize, Debug, Default)]
pub struct SyncRequest {
	#[serde(skip_serializing_if = "Option::is_none")]
	pub filter: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub since: Option<String>,
	pub full_state: bool,
	pub set_presence: String,
	pub timeout: u64,
}

#[derive(Serialize, Debug, Default)]
pub enum Presence {
	#[serde(rename = "offline")]
	Offline,
	#[serde(rename = "online")]
	#[default]
	Online,
	#[serde(rename = "unavailable")]
	Unavailable,
}

#[derive(Deserialize, Debug, Clone)]
pub struct SyncResponse {
	pub account_data: Option<Map<String, Value>>, // Option<AccountData>,
	pub device_lists: Option<Map<String, Value>>, // Option<DeviceLists>,
	pub device_one_time_keys_count: Option<Map<String, Value>>,
	pub next_batch: String,
	pub presence: Option<Map<String, Value>>, // Option<PresenceEvent>,
	pub rooms: Option<Rooms>,
	pub to_device: Option<Map<String, Value>>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct AccountData {
	pub events: Option<Vec<Map<String, Value>>>,
}
#[derive(Deserialize, Debug)]
pub struct PresenceEvent {
	pub events: Option<Vec<Map<String, Value>>>,
}
#[derive(Deserialize, Debug, Clone)]
pub struct Ephemeral {
	pub events: Option<Vec<Map<String, Value>>>,
}
#[derive(Deserialize, Debug, Clone)]
pub struct State {
	pub events: Option<Vec<ClientEventWithoutRoomId>>,
}
#[derive(Deserialize, Debug, Clone)]
pub struct ClientEventWithoutRoomId {
	pub content: Map<String, Value>,
	pub event_id: Option<String>,
	pub origin_server_ts: u64,
	pub sender: String,
	pub state_key: Option<String>,
	pub r#type: String,
	pub unsigned: Option<Box<UnsignedData>>,
}
#[derive(Deserialize, Debug, Clone)]
pub struct UnsignedData {
	pub age: Option<u64>,
	pub prev_content: Option<Map<String, Value>>,
	pub redacted_because: Option<ClientEventWithoutRoomId>,
	pub transaction_id: Option<String>,
}
#[derive(Deserialize, Debug, Clone)]
pub struct RoomSummary {
	#[serde(rename = "m.heroes")]
	pub heroes: Option<Vec<String>>,
	#[serde(rename = "m.invited_member_count")]
	pub invited_member_count: Option<u64>,
	#[serde(rename = "m.joined_member_count")]
	pub joined_member_count: Option<u64>,
}
#[derive(Deserialize, Debug, Clone)]
pub struct Timeline {
	pub events: Vec<ClientEventWithoutRoomId>,
	pub limited: Option<bool>,
	pub prev_batch: Option<String>,
}
#[derive(Deserialize, Debug, Clone)]
pub struct UnreadNotificationCounts {
	pub highlight_count: u64,
	pub notification_count: u64,
}
#[derive(Deserialize, Debug, Clone)]
pub struct ThreadNotificationCounts {
	pub highlight_count: u64,
	pub notification_count: u64,
}
#[derive(Deserialize, Debug)]
pub struct DeviceLists {
	pub changed: Option<Vec<String>>,
	pub left: Option<Vec<String>>,
}
#[derive(Deserialize, Debug, Clone)]
pub struct Rooms {
	pub invite: Option<HashMap<String, InvitedRoom>>,
	pub join: Option<HashMap<String, JoinedRoom>>,
	pub knock: Option<HashMap<String, KnockedRoom>>,
	pub leave: Option<HashMap<String, LeftRoom>>,
}
#[derive(Deserialize, Debug, Clone)]
pub struct InvitedRoom {
	pub invited_state: Option<RoomState>,
}
#[derive(Deserialize, Debug, Clone)]
pub struct KnockedRoom {
	pub knock_state: Option<RoomState>,
}
#[derive(Deserialize, Debug, Clone)]
pub struct RoomState {
	pub events: Vec<StrippedState>,
}
#[derive(Deserialize, Debug, Clone)]
pub struct StrippedState {
	pub sender: String,
	pub r#type: String,
	pub state_key: String,
	pub content: Map<String, Value>,
}
#[derive(Deserialize, Debug, Clone)]
pub struct JoinedRoom {
	pub account_data: Option<AccountData>,
	pub ephemeral: Option<Ephemeral>,
	pub state: Option<State>,
	pub summary: Option<RoomSummary>,
	pub timeline: Option<Timeline>,
	pub unread_notifications: Option<UnreadNotificationCounts>,
	pub unread_thread_notifications: Option<ThreadNotificationCounts>,
}
#[derive(Deserialize, Debug, Clone)]
pub struct LeftRoom {
	pub account_data: Option<AccountData>,
	pub state: Option<State>,
	pub timeline: Option<Timeline>,
}
