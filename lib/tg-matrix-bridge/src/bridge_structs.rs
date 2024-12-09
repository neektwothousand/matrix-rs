use std::sync::LazyLock;

use anyhow::{
	bail,
	Context,
};
use matrix_sdk::ruma::events::{
	room::message::MessageType,
	AnyMessageLikeEventContent,
	OriginalMessageLikeEvent,
};
use serde::Deserialize;
use teloxide::{
	adaptors::Throttle,
	types::ChatId,
	Bot,
};

#[derive(Clone)]
pub enum TgMessageKind {
	Text,
	Photo,
	Sticker,
	Document,
}

#[derive(Deserialize)]
pub struct Bridge {
	pub mx_id: String,
	pub tg_id: i64,
}

#[derive(Default, Clone)]
pub struct BmTgData {
	pub bot: Option<Throttle<Bot>>,
	pub chat_id: Option<ChatId>,
	pub message: Vec<u8>,
	pub tg_message_kind: Option<TgMessageKind>,
	pub caption: Option<String>,
	pub is_preview_disabled: bool,
}

pub struct BmMxData<'a> {
	pub mx_event: &'a OriginalMessageLikeEvent<AnyMessageLikeEventContent>,
	pub mx_msg_type: &'a MessageType,
	pub room: matrix_sdk::Room,
}

pub static BM_FILE_PATH: LazyLock<&str> = LazyLock::new(|| "bridged_messages/");

pub type MatrixMedia = (String, Vec<u8>, MessageType);

pub trait GetMatrixMedia {
	fn get_media(
		client: matrix_sdk::Client,
		media: MessageType,
	) -> impl std::future::Future<Output = anyhow::Result<MatrixMedia>> + Send;
}

impl GetMatrixMedia for MatrixMedia {
	async fn get_media(
		client: matrix_sdk::Client,
		media: MessageType,
	) -> anyhow::Result<MatrixMedia> {
		match media {
			MessageType::File(m) => {
				let Ok(Some(media)) = client.media().get_file(&m, true).await else {
					bail!("");
				};
				let value = serde_json::to_value(&m)?;
				let body = value.get("body").context(dbg!("body not found"))?;
				Ok((body.to_string(), media, MessageType::File(m)))
			}
			MessageType::Image(m) => {
				let Ok(Some(media)) = client.media().get_file(&m, true).await else {
					bail!("");
				};
				Ok((m.body.clone(), media, MessageType::Image(m)))
			}
			MessageType::Audio(m) => {
				let Ok(Some(media)) = client.media().get_file(&m, true).await else {
					bail!("");
				};
				Ok((m.body.clone(), media, MessageType::Audio(m)))
			}
			MessageType::Video(m) => {
				let Ok(Some(media)) = client.media().get_file(&m, true).await else {
					bail!("");
				};
				Ok((m.body.clone(), media, MessageType::Video(m)))
			}
			_ => bail!(""),
		}
	}
}
