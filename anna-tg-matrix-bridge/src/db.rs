use matrix_sdk::ruma::OwnedEventId;
use serde::{Deserialize, Serialize};
use teloxide::types::{ChatId, MessageId};

#[derive(Serialize, Deserialize, Debug)]
pub struct BridgedMessage {
	pub matrix_id: OwnedEventId,
	pub telegram_id: (ChatId, MessageId),
}
