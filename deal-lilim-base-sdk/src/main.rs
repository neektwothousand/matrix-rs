use futures_util::TryStreamExt;
use matrix_sdk_base::{
	crypto::{store::MemoryStore, OlmMachine},
	once_cell::sync::Lazy,
	ruma::{
		api::client::{message::send_message_event, redact::redact_event},
		DeviceId, TransactionId,
	},
	ruma::{events::room::message::RoomMessageEventContent, RoomId, UserId},
	ruma::{presence::PresenceState, OwnedDeviceId},
	store::StoreConfig,
	BaseClient, Room,
};
use ruma_client::{http_client::Hyper, Client};
use serde::Deserialize;
use std::{
	fs::{self, File},
	io::Read,
	os::unix::net::{UnixListener, UnixStream},
	path::Path,
	sync::Arc,
};
use tokio::time::{sleep, Duration};
use tokio::{runtime::Runtime, spawn};

const DIS_SOCK: &str = "/tmp/dis-rs.sock";
const MUR_SOCK: &str = "/tmp/mur-rs.sock";

static CLIENT: Lazy<Client<Hyper>> = Lazy::new(|| {
	let rt = Runtime::new().unwrap();
	let handle = rt.handle();
	let client_async = Client::builder().build();
	handle.block_on(client_async).unwrap()
});

#[derive(Deserialize)]
struct Bot {
	name: String,
	password: String,
	room_id: String,
}

async fn delete_message(room: &'static Room, res: send_message_event::v3::Response) {
	let event_id = res.event_id;
	sleep(Duration::new(3600, 0)).await;
	let txn_id = TransactionId::new().to_owned();
	let redact_request =
		redact_event::v3::Request::new(room.room_id().to_owned(), event_id, txn_id);
	CLIENT.send_request(redact_request).await.unwrap();
}

fn read_stream(mut stream: UnixStream) -> String {
	let mut buf = vec![];
	stream.read_to_end(&mut buf).unwrap();
	String::from_utf8_lossy(buf.as_slice()).to_string()
}

async fn read_sock(room: &'static Room, client: &'static Client<Hyper>, socket: &str) {
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
			let room_id = room.room_id().to_owned();
			let txn_id = TransactionId::new().to_owned();
			let content = RoomMessageEventContent::text_plain(sock_message);
			let message_request =
				send_message_event::v3::Request::new(room_id, txn_id, &content).unwrap();
			let res = client.send_request(message_request).await;
			match res {
				Ok(res) => {
					delete_message(room, res).await;
				}
				Err(err) => eprintln!("{:?}", err),
			}
		});
		tokio::task::yield_now().await;
	}
}

async fn polling(bot: Bot) {
	let deal_device_id_file_str = "deal_device_id";
	let device_id = if let Ok(mut f) = File::open(deal_device_id_file_str) {
		let mut device_id_str = String::new();
		f.read_to_string(&mut device_id_str).unwrap();
		let device_id: OwnedDeviceId = device_id_str.as_str().into();
		device_id
	} else {
		DeviceId::new()
	};
	CLIENT
		.log_in(&bot.name, &bot.password, Some(&device_id), None)
		.await
		.unwrap();

	let store = Arc::new(MemoryStore::new());
	let user_id = UserId::parse(&bot.name).unwrap();
	OlmMachine::with_store(&user_id, &device_id, store.clone())
		.await
		.unwrap();
	let store_config = StoreConfig::new().crypto_store(store);
	let base_client = BaseClient::with_store_config(store_config);

	let room_id = RoomId::parse(&bot.room_id).unwrap();
	let room: &'static Room = Box::leak(Box::new(base_client.get_room(&room_id).unwrap()));
	let sockets = [DIS_SOCK, MUR_SOCK];
	for socket in sockets {
		if Path::new(socket).exists() {
			fs::remove_file(socket).unwrap();
		}
		spawn(read_sock(room, &CLIENT, socket));
	}

	let filter = None;
	let since = String::new();
	let set_presence = PresenceState::Online;
	let timeout = Some(Duration::new(30, 0));
	let mut stream = Box::pin(CLIENT.sync(filter, since, set_presence, timeout));
	loop {
		stream.try_next().await.unwrap();
		sleep(Duration::new(10, 0)).await;
	}
}

#[tokio::main]
async fn main() {
	let bot: Bot = serde_yaml::from_reader(File::open("deal.yaml").unwrap()).unwrap();
	polling(bot).await;
}
