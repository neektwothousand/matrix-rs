use std::sync::Arc;

use anyhow::bail;

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

pub async fn get_tg_bot() -> teloxide::Bot {
	let token = std::fs::read_to_string("tg_token").unwrap();
	Bot::new(token)
}

pub async fn tg_text_handler(
	msg: Message,
	bot: Arc<Bot>,
	matrix_client: Client,
) -> anyhow::Result<()> {
	let Some(user) = msg.from() else {
		bail!("");
	};
	let Some(text) = msg.text() else {
		bail!("");
	};
	let text = if let Some(reply) = msg.reply_to_message() {
		if let Some(reply_text) = reply.text() {
			format!("{}\n{}: {text}", reply_text, user.first_name)
		} else if let Some(photo) = reply.photo() {
			let photo = photo.last().unwrap();
			let file_id = &photo.file.id;
			let file = bot.get_file(file_id).await.unwrap();
			let url = format!(
				"https://api.telegram.org/file/bot{}/{}",
				bot.token(),
				file.path
			);
			format!("{url}\n{}:{text}", user.first_name)
		} else {
			format!(
				"https://t.me/c/{}/{}\n{}:{text}",
				reply.chat.id, reply.id, user.first_name
			)
		}
	} else {
		format!("{}: {text}", user.first_name)
	};
	tg_text_matrix(&text, matrix_client).await;
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
	tg_photo_matrix(photo_file_path, msg.caption(), matrix_client).await;
	anyhow::Ok(())
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
	let text = format!("telegram:\n{}", text);
	let message = RoomMessageEventContent::text_plain(text);
	matrix_room.send(message).await.unwrap();
}

async fn tg_photo_matrix(
	photo_file_path: String,
	caption: Option<&str>,
	matrix_client: matrix_sdk::Client,
) {
	let matrix_room = matrix_client
		.get_room(&RoomId::parse(MATRIX_CHAT_ID).unwrap())
		.unwrap();
	let extension_str = std::path::Path::new(&photo_file_path).extension().unwrap();
	let content_type = match extension_str.to_str().unwrap() {
		"jpg" | "jpeg" => "image/jpeg".parse::<mime::Mime>().unwrap(),
		_ => return,
	};
	let photo = std::fs::read(photo_file_path).unwrap();
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
	let message = RoomMessageEventContent::new(MessageType::Image(image_message));
	matrix_room.send(message).await.unwrap();

	if let Some(caption) = caption {
		let message = RoomMessageEventContent::text_plain(caption);
		matrix_room.send(message).await.unwrap();
	}
}
