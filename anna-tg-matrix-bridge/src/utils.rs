use std::sync::Arc;

use anyhow::{bail, Context};

use matrix_sdk::{
	ruma::{
		events::room::message::{ImageMessageEventContent, MessageType, RoomMessageEventContent},
		RoomId,
	},
	Client,
};

use teloxide::{
	net::Download,
	payloads::{SendDocumentSetters, SendMessageSetters},
	requests::Requester,
	types::{InputFile, Message},
	Bot,
};

const MATRIX_CHAT_ID: &str = "!vUWLFTSVVBjhMouZpF:matrix.org";
const TG_CHAT_ID: i64 = -1001402125530;

pub type MatrixMedia = (String, Vec<u8>);

pub trait GetMatrixMedia {
	fn get_media(
		client: Client,
		media: MessageType,
	) -> impl std::future::Future<Output = anyhow::Result<MatrixMedia>> + Send;
}

impl GetMatrixMedia for MatrixMedia {
	async fn get_media(client: Client, media: MessageType) -> anyhow::Result<MatrixMedia> {
		match media {
			MessageType::File(m) => {
				let Ok(Some(media)) = client.media().get_file(m.clone(), true).await else {
					bail!("");
				};
				Ok((m.body, media))
			}
			MessageType::Image(m) => {
				let Ok(Some(media)) = client.media().get_file(m.clone(), true).await else {
					bail!("");
				};
				Ok((m.body, media))
			}
			MessageType::Audio(m) => {
				let Ok(Some(media)) = client.media().get_file(m.clone(), true).await else {
					bail!("");
				};
				Ok((m.body, media))
			}
			MessageType::Video(m) => {
				let Ok(Some(media)) = client.media().get_file(m.clone(), true).await else {
					bail!("");
				};
				Ok((m.body, media))
			}
			_ => bail!(""),
		}
	}
}

pub async fn get_matrix_media(
	client: Client,
	message_type: MessageType,
) -> anyhow::Result<(String, Vec<u8>)> {
	let Ok(media) =
		<(String, Vec<u8>) as GetMatrixMedia>::get_media(client.clone(), message_type.clone())
			.await
	else {
		bail!("");
	};
	Ok(media)
}

pub async fn get_tg_bot() -> teloxide::Bot {
	let token = std::fs::read_to_string("tg_token").unwrap();
	Bot::new(token)
}

pub async fn tg_text_handler(
	msg: Message,
	_bot: Arc<Bot>,
	matrix_client: Client,
) -> anyhow::Result<()> {
	let Some(user) = msg.from() else {
		bail!("");
	};
	let Some(text) = msg.text() else {
		bail!("");
	};
	let to_matrix_text = if let Some(reply) = msg.reply_to_message() {
		let mut chat_link = reply.chat.id.to_string();
		if let Some(reply_text) = reply.text() {
			format!("> {}\n{}: {text}", reply_text, user.first_name)
		} else {
			format!(
				"https://t.me/c/{}/{}\n{}:{text}",
				chat_link.drain(4..).as_str(),
				reply.id,
				user.first_name
			)
		}
	} else {
		format!("{}: {text}", user.first_name)
	};
	tg_text_matrix(&to_matrix_text, matrix_client).await;
	anyhow::Ok(())
}

pub async fn tg_photo_handler(
	msg: Message,
	bot: Arc<Bot>,
	matrix_client: Client,
) -> anyhow::Result<()> {
	let Some(user) = msg.from() else {
		bail!("");
	};
	let Some(media_photo) = msg.photo() else {
		bail!("");
	};
	let photo_id = &media_photo.last().unwrap().file.id;
	let photo_path = bot.get_file(photo_id).await.unwrap().path;
	let photo_file_path = format!("/tmp/{}:{}:{}.jpg", user.id, msg.id, msg.chat.id);
	let mut photo_dst = tokio::fs::File::create(&photo_file_path).await.unwrap();
	bot.download_file(&photo_path, &mut photo_dst)
		.await
		.unwrap();
	let sender = format!("from {}:", user.first_name.clone());
	tg_photo_matrix(photo_file_path, sender, msg.caption(), matrix_client).await?;
	Ok(())
}

pub async fn matrix_text_tg(text: String, bot: &Bot, preview: bool) {
	let chat_id = teloxide::types::ChatId(TG_CHAT_ID);
	match bot
		.send_message(chat_id, text)
		.disable_web_page_preview(preview)
		.await
	{
		Ok(_) => (),
		Err(e) => eprintln!("{:?}", e),
	};
}

pub async fn matrix_file_tg(file: Vec<u8>, file_name: String, caption: &str, bot: &Bot) {
	let chat_id = teloxide::types::ChatId(TG_CHAT_ID);
	match bot
		.send_document(chat_id, InputFile::memory(file).file_name(file_name))
		.caption(caption)
		.await
	{
		Ok(_) => (),
		Err(e) => eprintln!("{:?}", e),
	}
}

async fn tg_text_matrix(text: &str, matrix_client: matrix_sdk::Client) {
	let matrix_room = matrix_client
		.get_room(&RoomId::parse(MATRIX_CHAT_ID).unwrap())
		.unwrap();
	let message = RoomMessageEventContent::text_plain(text);
	matrix_room.send(message).await.unwrap();
}

async fn tg_photo_matrix(
	photo_file_path: String,
	sender: String,
	caption: Option<&str>,
	matrix_client: matrix_sdk::Client,
) -> anyhow::Result<()> {
	let matrix_room = matrix_client
		.get_room(&RoomId::parse(MATRIX_CHAT_ID)?)
		.context("")?;
	let extension_str = std::path::Path::new(&photo_file_path)
		.extension()
		.context("")?;
	let content_type = match extension_str.to_str().unwrap() {
		"jpg" | "jpeg" => "image/jpeg".parse::<mime::Mime>().unwrap(),
		_ => bail!(""),
	};
	let photo = std::fs::read(photo_file_path)?;
	let mxc_uri = matrix_client
		.media()
		.upload(&content_type, photo)
		.await
		.unwrap()
		.content_uri;
	let image_message = ImageMessageEventContent::new(
		"tg_photo".to_string(),
		matrix_sdk::ruma::events::room::MediaSource::Plain(mxc_uri),
	);
	let message = RoomMessageEventContent::text_plain(sender);
	matrix_room.send(message).await?;
	let message = RoomMessageEventContent::new(MessageType::Image(image_message));
	matrix_room.send(message).await?;

	if let Some(caption) = caption {
		let message = RoomMessageEventContent::text_plain(caption);
		matrix_room.send(message).await?;
	}

	Ok(())
}
