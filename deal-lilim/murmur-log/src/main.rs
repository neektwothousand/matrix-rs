use futures::{
	channel::mpsc::{channel, Receiver},
	SinkExt, StreamExt,
};
use matrix_sdk::{
	config::SyncSettings,
	ruma::{events::room::message::RoomMessageEventContent, RoomId, UserId},
	Client,
};
use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use regex::Regex;
use serde::Deserialize;
use std::{
	fs::File,
	io::{Read, Write},
	path::Path,
};

#[derive(Deserialize)]
struct User {
	name: String,
	password: String,
	room_id: String,
}

fn async_watcher() -> notify::Result<(RecommendedWatcher, Receiver<notify::Result<Event>>)> {
	let (mut tx, rx) = channel(1);

	// Automatically select the best implementation for your platform.
	// You can also access each implementation directly e.g. INotifyWatcher.
	let watcher = RecommendedWatcher::new(
		move |res| {
			futures::executor::block_on(async {
				tx.send(res).await.unwrap();
			})
		},
		Config::default(),
	)?;

	Ok((watcher, rx))
}

async fn async_watch<P: AsRef<Path>>(path: P, user: &User, client: &Client) -> notify::Result<()> {
	let (mut watcher, mut rx) = async_watcher()?;

	// Add a path to be watched. All files and directories at that path and
	// below will be monitored for changes.
	watcher.watch(path.as_ref(), RecursiveMode::Recursive)?;

	while let Some(res) = rx.next().await {
		match res {
			Ok(event) => {
				event_handler(event, client, user).await;
			}
			Err(e) => println!("watch error: {:?}", e),
		}
	}

	Ok(())
}

async fn event_handler(ev: Event, client: &Client, user: &User) {
	match ev.kind {
		notify::EventKind::Modify(_) => {
			let mut buf = String::new();
			File::open("murmur.log")
				.unwrap()
				.read_to_string(&mut buf)
				.unwrap();
			let mut last = buf.lines().last().unwrap().to_string();
			if !last.contains("Authenticated") && !last.contains("Connection closed") {
				return;
			}
			if last.contains("<0:(-1)>") {
				return;
			}
			let Some(split) = last.split_once("=>") else {
				return;
			};
			last = split.1.trim().to_string();
			let r = Regex::new(r"<[0-9]*").unwrap();
			last = r.replace(&last, "").to_string();
			let r = Regex::new(r"\(-[0-9]\):|\([0-9]\):").unwrap();
			last = r.replace(&last, "").to_string();
			let r = Regex::new(r":").unwrap();
			last = r.replace(&last, "").to_string();
			let r = Regex::new(r"\([0-9]\)>|\(-[0-9]\)>").unwrap();
			last = r.replace(&last, "").to_string();
			let r = Regex::new(r":.*").unwrap();
			last = r.replace(&last, "").to_string();
			last = format!("(murmur) {last}");

			let room_id = RoomId::parse(&user.room_id).unwrap();
			let joined_room = client.get_room(&room_id).unwrap();
			let content = RoomMessageEventContent::text_plain(last);
			let _ = joined_room.send(content).await;
		}
		_ => (),
	}
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
	let user: &'static mut User = Box::leak(Box::new(
		serde_yaml::from_reader(File::open("deal.yaml").unwrap()).unwrap(),
	));
	let user_id = UserId::parse(user.name.as_str()).unwrap();
	let client = Box::leak(Box::new(
		Client::builder()
			.sqlite_store("deal_sqlite_store", None)
			.server_name(user_id.server_name())
			.build()
			.await
			.unwrap(),
	));

	let login_builder = client
		.matrix_auth()
		.login_username(&user_id, &user.password);

	let deal_device_id_file_str = "deal_device_id";
	if let Ok(mut f) = File::open(deal_device_id_file_str) {
		let mut device_id_str = String::new();
		f.read_to_string(&mut device_id_str).unwrap();
		login_builder
			.device_id(&device_id_str)
			.send()
			.await
			.unwrap();
	} else {
		let response = login_builder.send().await.unwrap();
		let mut f = File::create(deal_device_id_file_str).unwrap();
		f.write_all(response.device_id.as_bytes()).unwrap();
	}
	client.sync_once(SyncSettings::default()).await?;

	let path = "murmur.log";
	let (async_watch, client_sync) = tokio::join!(
		async_watch(path, user, client),
		client.sync(SyncSettings::default())
	);

	async_watch?;
	client_sync?;
	return Ok(());
}
