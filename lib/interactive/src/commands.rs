use std::fs;
use std::io::Seek;
use std::io::Write;
use std::io::{
	self,
};
use std::path::Path;
use std::process::Command;
use std::process::Stdio;

use zip::write::FileOptionExtension;
use zip::write::FileOptions;
use zip::ZipWriter;
use zip::{
	self,
};

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

fn chars_bytes_usize_sum(vec_str: &[&str]) -> Vec<usize> {
	let mut list_n: Vec<usize> = vec![0, 0];
	let mut n: usize = 0;
	for x in 0..vec_str.len() {
		for y in vec_str[x].as_bytes() {
			n += *y as usize;
		}
		list_n[x] = n;
		n = 0;
	}
	list_n
}

fn sign_split_vec_str<'a>(
	reply_text: &'a str,
	signs: &[&'a str],
) -> Option<(Vec<&'a str>, &'a str)> {
	let mut vec_str: Option<(Vec<&str>, &str)> = None;
	for sign in signs {
		if reply_text.contains(sign) {
			if let Some(split) = reply_text.split_once(sign) {
				vec_str = Some((vec![split.0, split.1], *sign));
				break;
			}
		}
	}
	vec_str
}

fn zip_file<T: FileOptionExtension, W: Write + Seek>(
	zip: &mut zip::ZipWriter<W>,
	file: &str,
	options: FileOptions<'_, T>,
) {
	let Ok(read) = fs::read(file) else {
		return;
	};
	zip.start_file(file, options).unwrap();
	let buf = &read[..];
	zip.write_all(buf).unwrap();
}

fn zip_rec<T: FileOptionExtension + Clone, W: Write + Seek, P: AsRef<Path>>(
	out_zip_writer: W,
	path: P,
	options: &FileOptions<'_, T>,
) -> io::Result<ZipWriter<W>> {
	let mut zip_writer = zip::ZipWriter::new(out_zip_writer);
	zip_rec_aux(&mut zip_writer, path, options)?;
	Ok(zip_writer)
}

fn zip_rec_aux<T: FileOptionExtension + Clone, W: Write + Seek, P: AsRef<Path>>(
	zip: &mut zip::ZipWriter<W>,
	path: P,
	options: &FileOptions<'_, T>,
) -> io::Result<()> {
	for maybe_dir_entry in fs::read_dir(path)? {
		let dir_entry = maybe_dir_entry?;

		let file_type = dir_entry.file_type()?;
		if file_type.is_dir() {
			zip.add_directory_from_path(dir_entry.path(), options.clone())?;
			zip_rec_aux(zip, dir_entry.path(), options)?;
		} else if file_type.is_file() {
			let mut file = fs::File::open(dir_entry.path())?;
			zip.start_file_from_path(dir_entry.path(), options.clone()).unwrap();
			std::io::copy(&mut file, zip)?;
		}
	}
	Ok(())
}

fn cmd(command: &str, args: Vec<&str>, stdin_str: Option<String>) -> String {
	if let Some(stdin_str) = stdin_str {
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
	} else {
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

async fn get_original_text(reply_event: &TimelineEvent, room: &Room) -> Option<String> {
	let unsigned_event =
		reply_event.kind.raw().deserialize_as::<UnsignedRoomRedactionEvent>().ok()?;
	let replace = unsigned_event.unsigned.relations.replace;
	if let Some(replace) = replace {
		let timeline_event = room.event(&replace.event_id, None).await.ok()?;
		let room_message = timeline_event.kind.raw().deserialize_as::<RoomMessageEvent>().ok()?;
		let original_message = room_message.as_original()?;
		let relation = original_message.content.relates_to.clone()?;
		let Relation::Replacement(Replacement {
			new_content,
			..
		}) = relation
		else {
			return None;
		};
		match &new_content.msgtype {
			MessageType::Text(text_message) => Some(text_message.body.clone()),
			_ => None,
		}
	} else if let Ok(message) = reply_event.kind.raw().deserialize_as::<RoomMessageEvent>() {
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
	let Ok(reply_event) = room.event(&reply_id, None).await else {
		return None;
	};
	let reply_text = get_original_text(&reply_event, room).await?;

	Some(reply_text)
}

fn ddurandom(arg: &str, limit: u64) -> String {
	let tr_arg = match arg {
		"digit" => "\"[:digit:]\"",
		"alnum" => "\"[:alnum:]\"",
		"alpha" => "\"[:alpha:]\"",
		"graph" => "\"[:graph:]\"",
		_ => "\"[:print:]\"",
	};
	let bash_args = vec![format!(
		"dd if=/dev/urandom of=/dev/stdout status=none \\
			| tr -dc {tr_arg} | head -c {limit}"
	)];
	let res = Command::new("/bin/bash")
		.arg("-c")
		.args(bash_args)
		.stdout(Stdio::piped())
		.stderr(Stdio::piped())
		.spawn();
	if let Ok(spawn) = res {
		if let Ok(output) = spawn.wait_with_output() {
			format!(
				"{}\n{}",
				String::from_utf8_lossy(&output.stdout),
				String::from_utf8_lossy(&output.stderr),
			)
		} else {
			"error".to_string()
		}
	} else {
		"error".to_string()
	}
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
					.reply(original_message)
					.await
					.ok();
			};

			let user_id = original_message.sender.as_str();
			let authorized_users = [
				"@neek:matrix.archneek.me",
				"@lakeotp:matrix.archneek.me",
				"@slybianco:matrix.archneek.me",
			];
			let text = if authorized_users.iter().any(|&x| x == user_id) {
				cmd("/bin/bash", vec!["-c", args], None)
			} else {
				return SendMessage::text(room, &format!("{user_id} permission denied"))
					.reply(original_message)
					.await
					.ok();
			};
			SendMessage::text(room, &text).reply(original_message).await.ok()
		}
		"!ddurandom" | "!random" | "!rand" => {
			let arg = args.next().unwrap_or("alnum");
			let limit = args.next().unwrap_or("20").parse::<u64>().unwrap_or(20);
			let output = ddurandom(arg, limit);
			SendMessage::text(room, &output).reply(original_message).await.ok()
		}
		"!ping" => SendMessage::text(room, "pong").reply(original_message).await.ok(),
		"!sed" => {
			let args = match text.split_once(' ') {
				Some(split) => split.1,
				None => return None,
			};
			let stdin_str = reply_text?;
			let text = cmd("sed", vec!["--sandbox", args], Some(stdin_str));
			SendMessage::text(room, &text).reply(original_message).await.ok()
		}
		"!zip" | "!source" => {
			let tmp_id = format!("{}{}", original_message.room_id, original_message.event_id);
			let zip_name = format!("source{tmp_id}.zip");
			let Ok(file) = fs::OpenOptions::new()
				.write(true)
				.truncate(true)
				.create(true)
				.open(zip_name.clone())
			else {
				return None;
			};

			let options = zip::write::SimpleFileOptions::default()
				.compression_method(zip::CompressionMethod::Stored);
			let Ok(mut zip_writer) = zip_rec(file, "bot/", &options) else {
				return None;
			};

			zip_file(&mut zip_writer, "Cargo.toml", options);
			zip_file(&mut zip_writer, "LICENSE", options);
			let _ = zip_rec_aux(&mut zip_writer, "lib/", &options);
			let _ = zip_writer.finish();

			let Ok(mime) = "application/zip".parse::<Mime>() else {
				return None;
			};
			let Ok(file_content) = fs::read(&zip_name) else {
				return None;
			};
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
			let (vec_str, sign) = sign_split_vec_str(&reply_text, &signs)?;
			let vec_usize = chars_bytes_usize_sum(&vec_str);

			let result = match sign.trim() {
				"x" | "*" => (vec_usize[0] * vec_usize[1]).to_string(),
				"+" => (vec_usize[0] + vec_usize[1]).to_string(),
				"-" => (vec_usize[0] as isize - vec_usize[1] as isize).to_string(),
				"/" => (vec_usize[0] as f64 / vec_usize[1] as f64).to_string(),
				_ => return None,
			};

			SendMessage::text(room, &result).reply(original_message).await.ok()
		}
		_ => None,
	}
}
