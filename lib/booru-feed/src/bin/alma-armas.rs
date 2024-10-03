use anyhow::Error;
use booru_feed::UserRoom;
use booru_feed::{get_booru_post_tags, get_booru_posts, read_users, send_feed_post};
use matrix_sdk::config::SyncSettings;
use matrix_sdk::room::Room;
use matrix_sdk::ruma::events::reaction::ReactionEventContent;
use matrix_sdk::ruma::events::reaction::SyncReactionEvent;
use matrix_sdk::ruma::events::room::message::AddMentions;
use matrix_sdk::ruma::events::room::message::ForwardThread;
use matrix_sdk::ruma::events::room::message::Relation;
use matrix_sdk::ruma::events::room::message::RoomMessageEvent;
use matrix_sdk::ruma::events::room::message::{
	MessageType, RoomMessageEventContent, SyncRoomMessageEvent,
};
use matrix_sdk::ruma::events::room::MediaSource;
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
use std::str::SplitWhitespace;
use std::sync::LazyLock;
use std::time::Duration;
use teloxide::payloads::SendPhotoSetters;
use teloxide::prelude::Requester;
use teloxide::types::{ChatId, InputFile};
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use url::Url;

#[derive(Deserialize)]
struct User {
	name: String,
	password: String,
}

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() {
	let user: User = serde_yaml::from_reader(std::fs::File::open("alma.yaml").unwrap()).unwrap();
	let user_id = UserId::parse(&user.name).unwrap();
	let client = Client::builder()
		.handle_refresh_tokens()
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
	let sync_settings = SyncSettings::default().timeout(Duration::from_millis(100));
	loop {
		let sync = client.sync(sync_settings.clone()).await;
		if sync.is_err() {
			eprintln!("{:?}", sync);
			continue;
		};
	}
}
