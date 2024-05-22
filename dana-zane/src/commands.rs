use std::fs;
use std::io::Seek;
use std::io::Write;

use matrix_sdk::deserialized_responses::TimelineEvent;
use matrix_sdk::room::Room;
use matrix_sdk::ruma::api::client::message::send_message_event::v3::Response;
use matrix_sdk::ruma::events::relation::Replacement;
use matrix_sdk::ruma::events::room::message::MessageType;
use matrix_sdk::ruma::events::room::message::Relation;
use matrix_sdk::ruma::events::room::message::RoomMessageEvent;
use matrix_sdk::ruma::events::room::message::RoomMessageEventContent;
use matrix_sdk::ruma::events::room::message::TextMessageEventContent;
use matrix_sdk::ruma::events::OriginalSyncMessageLikeEvent;
use matrix_sdk::ruma::events::UnsignedRoomRedactionEvent;
use matrix_sdk::ruma::OwnedRoomId;

use matrix_sdk::ruma::events::room::message::sanitize::remove_plain_reply_fallback;
use mime::Mime;

use crate::utils::SendMessage;

fn chars_bytes_usize_sum(vec_str: Vec<&str>) -> Vec<usize> {
	let mut list_n: Vec<usize> = vec![0, 0];
	let mut n: usize = 0;
	for x in 0..vec_str.len() {
		for y in vec_str[x].as_bytes().iter() {
			n += *y as usize;
		}
		list_n[x] = n;
		n = 0;
	}
	list_n
}

fn sign_split_vec_str<'a>(
	reply_text: &'a str,
	signs: Vec<&'a str>,
) -> Option<(Vec<&'a str>, &'a str)> {
	let mut vec_str: Option<(Vec<&str>, &str)> = None;
	for sign in signs.iter() {
		if reply_text.contains(sign) {
			match reply_text.split_once(sign) {
				Some(split) => {
					vec_str = Some((vec![split.0, split.1], *sign));
					break;
				}
				None => (),
			}
		}
	}
	vec_str
}

fn zip_file<W: Write + Seek>(zip: &mut zip::ZipWriter<W>, file: &str) {
	let read = match std::fs::read(file) {
		Ok(read) => read,
		Err(_) => return,
	};
	let options =
		zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Stored);
	zip.start_file(file, options).unwrap();
	let buf = &read[..];
	zip.write_all(buf).unwrap();
}

fn cmd(command: &str, args: Vec<&str>, stdin_str: Option<String>) -> String {
	use std::process::{Command, Stdio};
	match stdin_str {
		Some(stdin_str) => {
			let mut cmd = Command::new(command)
				.args(args)
				.stdin(Stdio::piped())
				.stdout(Stdio::piped())
				.stderr(Stdio::piped())
				.spawn()
				.unwrap();
			let mut stdin = cmd.stdin.take().unwrap();
			std::thread::spawn(move || {
				stdin.write_all(stdin_str.as_bytes()).ok();
			});
			let output = cmd.wait_with_output().unwrap();
			format!(
				"{}{}",
				String::from_utf8_lossy(&output.stdout),
				String::from_utf8_lossy(&output.stderr)
			)
		}
		None => {
			let cmd = Command::new(command)
				.args(args)
				.stdout(Stdio::piped())
				.stderr(Stdio::piped())
				.spawn()
				.unwrap();
			let output = cmd.wait_with_output().unwrap();
			format!(
				"{}{}",
				String::from_utf8_lossy(&output.stdout),
				String::from_utf8_lossy(&output.stderr)
			)
		}
	}
}

async fn get_original_text(reply_event: &TimelineEvent, room: &Room) -> Option<String> {
	let unsigned_event = reply_event
		.event
		.deserialize_as::<UnsignedRoomRedactionEvent>()
		.ok()?;
	let replace = unsigned_event.unsigned.relations.replace;
	if let Some(replace) = replace {
		let timeline_event = room.event(&replace.event_id).await.ok()?;
		let room_message = timeline_event
			.event
			.deserialize_as::<RoomMessageEvent>()
			.ok()?;
		let original_message = room_message.as_original()?;
		let relation = original_message.content.relates_to.clone()?;
		let new_content = match relation {
			Relation::Replacement(Replacement { new_content, .. }) => new_content,
			_ => return None,
		};
		match &new_content.msgtype {
			MessageType::Text(text_message) => Some(text_message.body.clone()),
			_ => None,
		}
	} else if let Ok(message) = reply_event.event.deserialize_as::<RoomMessageEvent>() {
		let original = message.as_original()?;
		let MessageType::Text(ref text_message) = original.content.msgtype else {
			return None;
		};
		match original.content.relates_to {
			Some(Relation::Reply { .. }) => {
				return Some(remove_plain_reply_fallback(&text_message.body).to_string());
			}
			_ => {
				return Some(text_message.body.clone());
			}
		}
	} else {
		return None;
	}
}

async fn get_reply_text(
	original_message: &OriginalSyncMessageLikeEvent<RoomMessageEventContent>,
	room: &Room,
) -> Option<String> {
	let reply_id = match original_message.content.relates_to.clone() {
		Some(Relation::Reply { in_reply_to }) => in_reply_to.event_id,
		Some(_) | None => return None,
	};
	let Ok(reply_event) = room.event(&reply_id).await else {
		return None;
	};
	let Some(reply_text) = get_original_text(&reply_event, room).await else {
		return None;
	};

	Some(reply_text)
}

pub async fn match_command(
	room: &Room,
	command: &TextMessageEventContent,
	original_message: &OriginalSyncMessageLikeEvent<RoomMessageEventContent>,
) -> Option<Response> {
	let is_reply = if let Some(_reply) = &original_message.content.relates_to {
		true
	} else {
		false
	};
	let reply_text = get_reply_text(original_message, room).await;
	let text = if is_reply {
		remove_plain_reply_fallback(&command.body)
	} else {
		&command.body
	};
	let mut args = text.split_whitespace();

	let original_message = &original_message
		.clone()
		.into_full_event(OwnedRoomId::from(room.room_id()));

	let Some(command) = args.next() else {
		return None;
	};
	match command.to_lowercase().as_str() {
		"!dice" => {
			let Some(arg) = args.next() else {
				return None;
			};
			let Ok(number) = arg.parse::<u64>() else {
				return None;
			};
			use std::collections::HashMap;
			let mut hm = HashMap::new();
			for x in 1..=number {
				hm.insert(x.to_string(), x);
			}
			let text = hm.keys().next().unwrap();
			SendMessage::text(room, text)
				.await
				.reply(original_message)
				.await
				.ok()
		}
		"!ping" => SendMessage::text(room, "pong")
			.await
			.reply(original_message)
			.await
			.ok(),
		"!sed" => {
			let args = match text.split_once(' ') {
				Some(split) => split.1,
				None => return None,
			};
			let Some(reply_text) = reply_text else {
				return None;
			};
			let text = cmd("sed", vec!["--sandbox", args], Some(reply_text));
			SendMessage::text(room, &text)
				.await
				.reply(original_message)
				.await
				.ok()
		}
		"!zip" | "!source" => {
			let mut zip = {
				zip::ZipWriter::new(
					std::fs::OpenOptions::new()
						.write(true)
						.create(true)
						.open("source.zip")
						.unwrap(),
				)
			};

			zip_file(&mut zip, "Cargo.toml");
			zip_file(&mut zip, "LICENSE");
			for path in fs::read_dir("src/").unwrap() {
				zip_file(&mut zip, path.unwrap().path().to_str().unwrap());
			}

			zip.finish().unwrap();

			let mime = "application/zip".parse::<Mime>().unwrap();
			let file_content = fs::read("source.zip").unwrap();
			SendMessage::file(room.clone(), (mime, file_content))
				.await?
				.reply(original_message)
				.await
				.ok()
		}
		_ => None,
	}
}

pub async fn match_text(
	room: &Room,
	text_message: &TextMessageEventContent,
	original_message: &OriginalSyncMessageLikeEvent<RoomMessageEventContent>,
) -> Option<Response> {
	let is_reply = if let Some(_reply) = &original_message.content.relates_to {
		true
	} else {
		false
	};
	let text = if is_reply {
		remove_plain_reply_fallback(&text_message.body)
	} else {
		&text_message.body
	};
	let reply_text = get_reply_text(original_message, room).await;
	let original_message = &original_message
		.clone()
		.into_full_event(OwnedRoomId::from(room.room_id()));
	match text.to_lowercase().as_str() {
		"quanto fa" => {
			let Some(reply_text) = reply_text else {
				return None;
			};

			let signs = vec![" x ", " * ", " + ", " - ", " / ", "*", "+", "-", "/"];
			let (vec_str, sign) = sign_split_vec_str(&reply_text, signs)?;
			let vec_usize = chars_bytes_usize_sum(vec_str);

			let result = match sign.trim() {
				"x" | "*" => (vec_usize[0] * vec_usize[1]).to_string(),
				"+" => (vec_usize[0] + vec_usize[1]).to_string(),
				"-" => (vec_usize[0] as isize - vec_usize[1] as isize).to_string(),
				"/" => (vec_usize[0] as f64 / vec_usize[1] as f64).to_string(),
				_ => return None,
			};

			SendMessage::text(room, &result)
				.await
				.reply(original_message)
				.await
				.ok()
		}
		_ => None,
	}
}
