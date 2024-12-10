use matrix_sdk::ruma::OwnedEventId;
use serde::Deserialize;
use serde::Serialize;
use teloxide::types::ChatId;
use teloxide::types::MessageId;

#[derive(Serialize, Deserialize, Debug)]
pub struct BridgedMessage {
	pub matrix_id: OwnedEventId,
	pub telegram_id: (ChatId, MessageId),
}
