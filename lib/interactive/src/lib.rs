use crate::commands::{match_command, match_text};
use matrix_sdk::ruma::events::room::message::{MessageType, SyncRoomMessageEvent};
use matrix_sdk::ruma::events::SyncMessageLikeEvent;
use matrix_sdk::{Client, Room};
use std::sync::Arc;

mod commands;
mod utils;

pub async fn event_handler(client: Arc<Client>) {
	client.add_event_handler(
		move |ev: SyncRoomMessageEvent, room: Room, client: Client| async move {
			if ev.sender().as_str() == client.user_id().unwrap().as_str() {
				return;
			}
			if let SyncMessageLikeEvent::Original(original_message) = ev {
				if let (MessageType::Text(text), room) =
					(original_message.content.msgtype.clone(), room.clone())
				{
					let _ = match_command(&room, &text, &original_message).await;
					let _ = match_text(&room, &text, &original_message).await;
				};
			}
		},
	);
}
