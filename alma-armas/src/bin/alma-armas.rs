use anyhow::Error;
use lazy_static::lazy_static;
use matrix_sdk::config::SyncSettings;
use matrix_sdk::room::Room;
use matrix_sdk::ruma::events::reaction::ReactionEventContent;
use matrix_sdk::ruma::events::reaction::SyncReactionEvent;
use matrix_sdk::ruma::events::room::MediaSource;
use matrix_sdk::ruma::events::room::message::AddMentions;
use matrix_sdk::ruma::events::room::message::ForwardThread;
use matrix_sdk::ruma::events::room::message::Relation;
use matrix_sdk::ruma::events::room::message::RoomMessageEvent;
use matrix_sdk::ruma::events::room::message::{
	MessageType, RoomMessageEventContent, SyncRoomMessageEvent,
};
use matrix_sdk::ruma::events::{OriginalSyncMessageLikeEvent, SyncMessageLikeEvent};
use matrix_sdk::ruma::EventId;
use matrix_sdk::ruma::OwnedEventId;
use matrix_sdk::ruma::RoomId;
use matrix_sdk::ruma::UserId;
use matrix_sdk::Client;
use rand::seq::SliceRandom;
use serde::Deserialize;
use std::borrow::Cow;
use std::fs::read_to_string;
use std::io::Write;
use std::rc::Rc;
use std::str::SplitWhitespace;
use std::time::Duration;
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use url::Url;
use alma_armas::UserRoom;
use alma_armas::{get_booru_post_tags, get_booru_posts, read_users, send_feed_post};
use teloxide::types::{ChatId, InputFile};
use teloxide::prelude::Requester;
use teloxide::payloads::SendPhotoSetters;

#[derive(Deserialize)]
struct User {
	name: String,
	password: String,
}

const TGTOKEN_PATH: &str = "alma-armas/tgtoken";

lazy_static! {
	static ref TGTOKEN: String = read_to_string(TGTOKEN_PATH).unwrap();
	static ref TGBOT: teloxide::Bot = teloxide::Bot::new(TGTOKEN.clone());
}

fn get_url_query(is_url: bool, query: &str, user_id: &UserId) -> Option<(Url, bool)> {
	let inline_markup: bool;
	if is_url {
		let url = Url::parse(query).ok()?;
		let url_domain = url.domain()?;
		let url_query = if url_domain == "danbooru.donmai.us" {
			let segments = url.path_segments()?;
			let last_seg = segments.last()?;
			let post_id = last_seg.parse::<u64>().ok()?;
			format!("https://{}/posts/{post_id}.json", url_domain)
		} else {
			let post_id_query = url.query_pairs().find(|q| q.0 == Cow::Borrowed("id"))?; 
			format!(
				"https://{}/index.php?page=dapi&s=post&q=index&json=1&{}={}",
				url_domain, post_id_query.0, post_id_query.1
			)
		};

		let booru_users = read_users().unwrap();
		if let Some(_booru_user) = booru_users.iter().find(|&user| user.id == user_id.as_str()) {
			inline_markup = true;
		} else {
			inline_markup = false;
		}
		Some((Url::parse(&url_query).ok()?, inline_markup))
	} else {
		let url_query = format!(
			"https://gelbooru.com/{}&tags={query}",
			"index.php?page=dapi&s=post&q=index&json=1"
		);
		inline_markup = false;
		Some((Url::parse(&url_query).ok()?, inline_markup))
	}
}

async fn handle_message_event(
	event: SyncRoomMessageEvent,
	room: Room,
	client: Client,
) -> anyhow::Result<()> {
	if event.sender() == client.user_id().unwrap() {
		return Ok(());
	}
	let SyncMessageLikeEvent::Original(original_event) = event else {
		return Ok(());
	};
	let MessageType::Text(ref text) = original_event.content.msgtype else {
		return Ok(());
	};
	let mut args = text.body.split_whitespace();
	let command = args.next().unwrap();
	if let Some(send_text_plain) = match_command(original_event.clone(), command, &room, args).await
	{
		room.send(RoomMessageEventContent::text_plain(send_text_plain))
			.await?;
	}
	Ok(())
}

async fn match_command(
	event: OriginalSyncMessageLikeEvent<RoomMessageEventContent>,
	command: &str,
	room: &Room,
	mut args: SplitWhitespace<'_>,
) -> Option<String> {
	#[allow(clippy::single_match)]
	match command {
		"!booru" => {
			let message_user = event.sender;
			let message_arg = args.next()?;
			let is_url = Url::parse(message_arg).is_ok();
			let url_query = get_url_query(is_url, message_arg, &message_user)?;
			let domain = url_query.0.domain()?;
			let booru_posts = match get_booru_posts(url_query.0.as_str()).await {
				Ok(booru_posts) => booru_posts,
				Err(err) => {
					println!("{:?}", err);
					return None;
				}
			};
			let booru_posts = match booru_posts {
				Some(mut posts) => {
					let mut rng = rand::thread_rng();
					posts.shuffle(&mut rng);
					posts
				}
				None => return None,
			};

			let post = booru_posts.first()?;

			let hashtags = get_booru_post_tags(post, None).await;
			let source = if domain == "danbooru.donmai.us" {
				format!("https://{domain}/posts/{}", post.id)
			} else {
				format!(
					"https://{}/index.php?page=post&s=view&id={}",
					domain, post.id
				)
			};
			let caption = format!(
				"{} — {source}",
				hashtags.join(" ").replace(
					['\\', '!', '\'', ':', '{', '}', '+', '~', '(', ')', '.', ',', '/', '-'],
					""
				)
			);

			send_feed_post(room, post.clone(), &caption).await;
		}
		_ => (),
	}

	None
}

async fn handle_reaction_event(
	event: SyncReactionEvent,
	room: Room,
	client: Client,
) -> anyhow::Result<()> {
	if event.sender() == client.user_id().unwrap() {
		return Ok(());
	}
	let SyncMessageLikeEvent::Original(original_event) = event else {
		return Ok(());
	};
	match match_reaction(original_event, &room).await {
		Ok(_) => Ok(()),
		Err(err) => {
			eprintln!("{}", err);
			Ok(())
		}
	}
}

fn parse_caption_hashtags(caption: &str) -> Option<Vec<String>> {
	let mut hashtags: Vec<String> = vec![];
	let split = caption.split_whitespace();

	for word in split {
		if word.starts_with('#') {
			hashtags.push(word.to_string());
		}
	}

	if hashtags.is_empty() {
		None
	} else {
		Some(hashtags)
	}
}

async fn match_reaction(
	event: OriginalSyncMessageLikeEvent<ReactionEventContent>,
	room: &Room,
) -> anyhow::Result<()> {
	let reaction = event.content.relates_to.key;
	let caption_event_id = event.content.relates_to.event_id;
	let caption_event = room
		.event(&caption_event_id)
		.await?
		.event
		.deserialize_as::<RoomMessageEvent>()?;
	let caption_event = match caption_event.as_original() {
		Some(caption_event) => caption_event,
		None => {
			let err = format!("cannot get {:?} as original", &caption_event);
			return Err(Error::msg(err));
		}
	};
	let (reply_event, media_event_id) = if let Some(Relation::Reply { in_reply_to }) =
		caption_event.content.relates_to.as_ref()
	{
		let in_reply_event = in_reply_to.clone();
		let timeline_event = loop {
			let Ok(event) = room.event(&in_reply_to.event_id).await else {
				continue;
			};
			break event;
		};
		let Ok(Some(media_event_id)) = timeline_event.event.get_field::<OwnedEventId>("event_id")
		else {
			return Ok(());
		};
		let media_event_id = EventId::parse(media_event_id).unwrap();
		(in_reply_event.clone(), media_event_id)
	} else {
		return Err(Error::msg("no reply found"));
	};

	if reaction == "❌" {
		room.redact(&reply_event.event_id, None, None).await?;
		room.redact(&media_event_id, None, None).await?;
		return Ok(());
	}

	let users = read_users().unwrap();

	let Some(user) = users.iter().find(|&user| user.id == event.sender) else {
		return Err(Error::msg(format!("{} is not in user list", event.sender)));
	};

	let mut tags: Vec<String> = vec![];
	let caption_event_text = caption_event.content.body();
	if let Some(hashtags) = parse_caption_hashtags(caption_event_text) {
		for hashtag in hashtags {
			tags.push(hashtag);
		}
	}
	let mut source = String::new();
	if let Ok(url) = url::Url::parse(caption_event_text.split('—').last().unwrap().trim()) {
		source = url.to_string();
	}

	let data = if reaction == "✅" { "nsfw" } else { "sfw" };
	let mut user_room: Option<UserRoom> = None;
	for user_chat in &user.chats {
		if user_chat.rating == data {
			if let Some(whitelist) = &user_chat.whitelist {
				for whitelist_tag in whitelist {
					if tags.contains(whitelist_tag) {
						user_room = Some(user_chat.clone());
						break;
					}
				}
			} else {
				user_room = Some(user_chat.clone());
				break;
			}
		} else {
			continue;
		}
	}
	let Some(user_room) = user_room else {
		return Err(Error::msg("user not found"));
	};

	let mut caption = user_room.caption.clone();
	if let Some(link) = user_room.link.clone() {
		caption = format!("{caption} {}", link);
	}

	if user_room.has_tags {
		caption = format!("{caption}\n{} — {source}", tags.join(" "));
	}

	let to_room_id = room
		.client()
		.get_room(&RoomId::parse(user_room.id)?)
		.ok_or_else(|| eprintln!("room not found"))
		.unwrap();

	let media_event = loop {
		let Ok(event) = room.event(&media_event_id).await else {
			continue;
		};
		break event;
	};
	let media_event = media_event
		.event
		.deserialize_as::<RoomMessageEvent>()?
		.as_original()
		.unwrap()
		.to_owned();

	let tgnova = ChatId(-1001434279006);

	let request = if let MessageType::Image(image) = media_event.content.msgtype {
		let image_t = image.clone();
		let caption_t = caption.clone();
		tokio::spawn(async move {if let MediaSource::Plain(ref mxcuri) = image_t.source {
			match url::Url::parse(mxcuri.as_str()) {
				Ok(u) => {
					let domain = "https://matrix.org/_matrix/media/v3/download/matrix.org";
					let path = u.path();
					let u = url::Url::parse(format!("{}{}", domain, path).as_str()).unwrap();
					let tgfile = InputFile::memory(reqwest::get(u.clone()).await.unwrap().bytes().await.unwrap());
					let tgres = TGBOT.send_photo(tgnova, tgfile)
						.caption(caption_t).await;
					if tgres.is_err() {
						eprintln!("{:?}\n{:?}", tgres, u);
					}
				}
				Err(e) => eprintln!("{:?}", e),
			}
		}});
		RoomMessageEventContent::new(MessageType::Image(image))
	} else if let MessageType::Video(video) = media_event.content.msgtype {
		RoomMessageEventContent::new(MessageType::Video(video))
	} else {
		return Err(Error::msg(format!(
			"message type is: {:?}",
			media_event.content.msgtype
		)));
	};
	tokio::spawn(async move {
		let sent_media_event_id = to_room_id.send(request).await.unwrap().event_id;
		let original_message = to_room_id
			.event(&sent_media_event_id)
			.await.unwrap()
			.event
			.deserialize_as::<RoomMessageEvent>().unwrap();
		let forward_thread = ForwardThread::No;
		let add_mentions = AddMentions::No;
		let text_content = RoomMessageEventContent::text_plain(caption).make_reply_to(
			original_message.as_original().unwrap(),
			forward_thread,
			add_mentions,
		);
		let sent_text_event_id = to_room_id.send(text_content).await.unwrap().event_id;
		if let Some(queues) = &user_room.queue {
			for queue_chat_id in queues {
				let path = format!("alma-armas/db/queue/{}", to_room_id.room_id().as_str());
				std::fs::create_dir_all(path.clone()).unwrap();
				let mut queue_file = std::fs::File::options()
					.create(true)
					.append(true)
					.open(&format!("{path}/{queue_chat_id}"))
					.unwrap();
				writeln!(
					&mut queue_file,
					"{} {}",
					sent_media_event_id.as_str(),
					sent_text_event_id.as_str()
				)
				.unwrap();
			}
		}
	});

	room.redact(&media_event_id, None, None).await?;
	room.redact(&caption_event_id, None, None).await?;

	Ok(())
}

#[derive(Deserialize)]
struct User {
	name: String,
	password: String,
}
#[tokio::main]
async fn main() {
	let user: User = serde_yaml::from_reader(std::fs::File::open("alma.yaml").unwrap()).unwrap();
	let user_id = UserId::parse(&user.name).unwrap();
	let client = Client::builder()
		.sqlite_store("alma_sqlite_store", None)
		.server_name(user_id.server_name())
		.build()
		.await
		.unwrap();

	// First we need to log in.
	let login_builder = client
		.matrix_auth()
		.login_username(&user_id, &user.password);

	let alma_device_id_file_str = "alma_device_id";
	if let Ok(mut f) = File::open(alma_device_id_file_str).await {
		let mut device_id_str = String::new();
		f.read_to_string(&mut device_id_str).await.unwrap();

		login_builder
			.device_id(&device_id_str)
			.send()
			.await
			.unwrap();
	} else {
		let response = login_builder.send().await.unwrap();
		let mut f = File::create(alma_device_id_file_str).await.unwrap();
		f.write_all(response.device_id.as_bytes()).await.unwrap();
	}
	client.sync_once(SyncSettings::default()).await.unwrap();

	lazy_static::initialize(&TGBOT);

	client.add_event_handler(handle_message_event);
	client.add_event_handler(handle_reaction_event);
	loop {
		let sync = client.sync(SyncSettings::default()).await;
		if sync.is_err() {
			eprintln!("alma-armas http error, retrying");
			continue;
		};
	}
}
