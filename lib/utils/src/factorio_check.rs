use std::{
	net::{
		SocketAddr,
		ToSocketAddrs,
		UdpSocket,
	},
	time::Duration,
};

use serde::Deserialize;
use toml::from_str;

#[derive(Deserialize)]
struct FactorioAddrs {
	factorio_addrs: Vec<String>,
}

pub fn factorio_check() -> Option<String> {
	let msg = [0x22, 0x00, 0x00, 0x02, 0x00, 0x07, 0x19, 0x36, 0x01, 0x00, 0xb3, 0x24, 0x43, 0x66];
	let socket = UdpSocket::bind("0.0.0.0:0").unwrap();
	let port = 34197u16;

	let bot_data = std::fs::read_to_string("bot_data.toml").unwrap();
	let factorio_server_addrs: FactorioAddrs = from_str(&bot_data).unwrap();
	for addr in factorio_server_addrs.factorio_addrs {
		let Ok(mut to_socket_addrs) = (addr.clone(), port).to_socket_addrs() else {
			continue;
		};
		let Some(SocketAddr::V4(socket_addr)) = to_socket_addrs.next() else {
			continue;
		};

		socket.send_to(&msg, socket_addr).unwrap();
		let mut buf = [0; 64];
		socket.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
		
		let status: Option<String>;
		match socket.recv(&mut buf) {
			Ok(_) => status = Some(socket_addr.to_string()),
			Err(_) => status = None,
		}
		if status.is_some() {
			return status;
		}
	}
	None
}
