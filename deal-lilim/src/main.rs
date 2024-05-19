use std::{
	fs::{self, File},
	io::{Read, Write},
	os::unix::net::{UnixListener, UnixStream},
	path::Path,
};

use matrix_sdk::{
	config::SyncSettings,
	ruma::api::client::message::send_message_event,
	ruma::{events::room::message::RoomMessageEventContent, RoomId, UserId},
	Client, Room,
};
use serde::Deserialize;
use tokio::task::spawn;
use tokio::time::{sleep, Duration};

const DIS_SOCK: &str = "/tmp/dis-rs.sock";
const MUR_SOCK: &str = "/tmp/mur-rs.sock";

#[derive(Deserialize)]
struct User {
	name: String,
	password: String,
	room_id: String,
}

async fn delete_message(room: &Room, res: send_message_event::v3::Response) {
	let event_id = res.event_id;
	sleep(Duration::from_secs(86400)).await;
	let res = room.redact(&event_id, None, None).await;
	if let Err(res) = res {
		eprintln!("redact:\n{:#?}", res);
	}
}

fn read_stream(mut stream: UnixStream) -> String {
	let mut buf = vec![];
	stream.read_to_end(&mut buf).unwrap();

	String::from_utf8_lossy(buf.as_slice()).to_string()
}

async fn read_sock(room: &'static Room, socket: &str) {
	let unix_listener = UnixListener::bind(socket).unwrap();
	for stream in unix_listener.incoming() {
		let sock_message = match stream {
			Ok(stream) => read_stream(stream),
			Err(err) => {
				eprintln!("{:?}", err);
				continue;
			}
		};
		spawn(async move {
			println!("{}", &sock_message);
			let content = RoomMessageEventContent::text_plain(sock_message);
			match room.send(content).await {
				Ok(res) => {
					delete_message(&room, res).await;
				}
				Err(err) => eprintln!("{:?}", err),
			}
		});
		tokio::task::yield_now().await;
	}
}

#[tokio::main]
async fn main() {
	let user: User = serde_yaml::from_reader(File::open("deal.yaml").unwrap()).unwrap();
	let user_id = UserId::parse(user.name.as_str()).unwrap();
	let client = Client::builder()
		.sqlite_store("deal_sqlite_store", None)
		.server_name(user_id.server_name())
		.build()
		.await
		.unwrap();

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

	let room_id = RoomId::parse(&user.room_id).unwrap();
	let room: &'static Room = Box::leak(Box::new(client.get_room(&room_id).unwrap()));
	let sockets = [DIS_SOCK, MUR_SOCK];
	
	for socket in sockets {
		if Path::new(socket).exists() {
			fs::remove_file(socket).unwrap();
		}
		spawn(read_sock(&room, socket));
	}

	loop {
		let client_sync = client.sync(SyncSettings::default()).await;
		let Err(ref e) = client_sync else {
			continue;
		};
		eprintln!("{:?}", e);
	}
}
