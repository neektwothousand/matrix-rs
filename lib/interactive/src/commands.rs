use std::{
	fs,
	io::{
		Seek,
		Write,
		{
			self,
		},
	},
	path::Path,
};
use zip::{
	self,
	write::{
		FileOptionExtension,
		FileOptions,
	},
	ZipWriter,
};

use matrix_sdk::{
	deserialized_responses::TimelineEvent,
	room::Room,
	ruma::{
		api::client::message::send_message_event::v3::Response,
		events::{
			relation::Replacement,
			room::message::{
				MessageType,
				Relation,
				RoomMessageEvent,
				RoomMessageEventContent,
				TextMessageEventContent,
			},
			OriginalSyncMessageLikeEvent,
			UnsignedRoomRedactionEvent,
		},
		OwnedRoomId,
	},
};

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
			if let Some(split) = reply_text.split_once(sign) {
				vec_str = Some((vec![split.0, split.1], *sign));
				break;
			}
		}
	}
	vec_str
}

fn zip_file<'k, T: FileOptionExtension, W: Write + Seek>(
	zip: &mut zip::ZipWriter<W>,
	file: &str,
	options: FileOptions<'k, T>,
) {
	let read = match fs::read(file) {
		Ok(read) => read,
		Err(_) => return,
	};
	zip.start_file(file, options).unwrap();
	let buf = &read[..];
	zip.write_all(buf).unwrap();
}

fn zip_rec<'k, T: FileOptionExtension + Clone, W: Write + Seek, P: AsRef<Path>>(
	out_zip_writer: W,
	path: P,
	options: FileOptions<'k, T>,
) -> io::Result<ZipWriter<W>> {
	let mut zip_writer = zip::ZipWriter::new(out_zip_writer);
	zip_rec_aux(&mut zip_writer, path, options)?;
	Ok(zip_writer)
}

fn zip_rec_aux<'k, T: FileOptionExtension + Clone, W: Write + Seek, P: AsRef<Path>>(
	zip: &mut zip::ZipWriter<W>,
	path: P,
	options: FileOptions<'k, T>,
) -> io::Result<()> {
	for maybe_dir_entry in fs::read_dir(path)? {
		let dir_entry = maybe_dir_entry?;

		let file_type = dir_entry.file_type()?;
		if file_type.is_dir() {
			zip.add_directory_from_path(dir_entry.path(), options.clone())?;
			zip_rec_aux(zip, dir_entry.path(), options.clone())?;
		} else if file_type.is_file() {
			let mut file = fs::File::open(dir_entry.path())?;
			zip.start_file_from_path(dir_entry.path(), options.clone()).unwrap();
			std::io::copy(&mut file, zip)?;
		}
	}
	Ok(())
}

fn cmd(command: &str, args: Vec<&str>, stdin_str: Option<String>) -> String {
	use std::process::{
		Command,
		Stdio,
	};
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
	let unsigned_event = reply_event.event.deserialize_as::<UnsignedRoomRedactionEvent>().ok()?;
	let replace = unsigned_event.unsigned.relations.replace;
	if let Some(replace) = replace {
		let timeline_event = room.event(&replace.event_id).await.ok()?;
		let room_message = timeline_event.event.deserialize_as::<RoomMessageEvent>().ok()?;
		let original_message = room_message.as_original()?;
		let relation = original_message.content.relates_to.clone()?;
		let new_content = match relation {
			Relation::Replacement(Replacement {
				new_content,
				..
			}) => new_content,
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
		return match original.content.relates_to {
			Some(Relation::Reply {
				..
			}) => Some(remove_plain_reply_fallback(&text_message.body).to_string()),
			_ => Some(text_message.body.clone()),
		};
	} else {
		return None;
	}
}

async fn get_reply_text(
	original_message: &OriginalSyncMessageLikeEvent<RoomMessageEventContent>,
	room: &Room,
) -> Option<String> {
	let reply_id = match original_message.content.relates_to.clone() {
		Some(Relation::Reply {
			in_reply_to,
		}) => in_reply_to.event_id,
		Some(_) | None => return None,
	};
	let Ok(reply_event) = room.event(&reply_id).await else {
		return None;
	};
	let reply_text = get_original_text(&reply_event, room).await?;

	Some(reply_text)
}

pub async fn match_command(
	room: &Room,
	command: &TextMessageEventContent,
	original_message: &OriginalSyncMessageLikeEvent<RoomMessageEventContent>,
) -> Option<Response> {
	let is_reply = original_message.content.relates_to.is_some();
	let reply_text = get_reply_text(original_message, room).await;
	let text = if is_reply {
		remove_plain_reply_fallback(&command.body)
	} else {
		&command.body
	};
	let mut args = text.split_whitespace();

	let original_message =
		&original_message.clone().into_full_event(OwnedRoomId::from(room.room_id()));

	let command = args.next()?;
	match command.to_lowercase().as_str() {
		"!bin" => {
			let Some(args) = text.split_once(' ').map(|x| x.1) else {
				return SendMessage::text(room, "missing arguments!")
					.await
					.reply(original_message)
					.await
					.ok();
			};

			let user_id = original_message.sender.as_str();
			let authorized_users = vec![
				"@neek:matrix.archneek.me",
				"@lakeotp:matrix.archneek.me",
				"@slybianco:matrix.archneek.me",
			];
			let text = if authorized_users.iter().find(|&&x| x == user_id).is_some() {
				cmd("/bin/bash", vec!["-c", args], None)
			} else {
				return SendMessage::text(room, &format!("{} permission denied", user_id))
					.await
					.reply(original_message)
					.await
					.ok();
			};
			SendMessage::text(room, &text).await.reply(original_message).await.ok()
		}
		"!dice" => {
			let arg = args.next()?;
			let number = arg.parse::<u64>().ok()?;
			use std::collections::HashMap;
			let mut hm = HashMap::new();
			for x in 1..=number {
				hm.insert(x.to_string(), x);
			}
			let text = hm.keys().next().unwrap();
			SendMessage::text(room, text).await.reply(original_message).await.ok()
		}
		"!ping" => SendMessage::text(room, "pong").await.reply(original_message).await.ok(),
		"!sed" => {
			let args = match text.split_once(' ') {
				Some(split) => split.1,
				None => return None,
			};
			let stdin_str = reply_text?;
			let text = cmd("sed", vec!["--sandbox", args], Some(stdin_str));
			SendMessage::text(room, &text).await.reply(original_message).await.ok()
		}
		"!zip" | "!source" => {
			let tmp_id = format!("{}{}", original_message.room_id, original_message.event_id);
			let zip_name = format!("source{}.zip", tmp_id);
			let file = fs::OpenOptions::new()
				.write(true)
				.truncate(true)
				.create(true)
				.open(zip_name.clone())
				.unwrap();

			let options = zip::write::SimpleFileOptions::default()
				.compression_method(zip::CompressionMethod::Stored);
			let mut zip_writer = zip_rec(file, "bot/", options).unwrap();

			zip_file(&mut zip_writer, "Cargo.toml", options);
			zip_file(&mut zip_writer, "LICENSE", options);
			zip_rec_aux(&mut zip_writer, "lib/", options).unwrap();

			zip_writer.finish().unwrap();

			let mime = "application/zip".parse::<Mime>().unwrap();
			let file_content = fs::read(&zip_name).unwrap();
			SendMessage::file(room.clone(), zip_name, (mime, file_content))
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
	let is_reply = original_message.content.relates_to.is_some();
	let text = if is_reply {
		remove_plain_reply_fallback(&text_message.body)
	} else {
		&text_message.body
	};
	let reply_text = get_reply_text(original_message, room).await;
	let original_message =
		&original_message.clone().into_full_event(OwnedRoomId::from(room.room_id()));
	match text.to_lowercase().as_str() {
		"quanto fa" => {
			let reply_text = reply_text?;

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

			SendMessage::text(room, &result).await.reply(original_message).await.ok()
		}
		_ => None,
	}
}
