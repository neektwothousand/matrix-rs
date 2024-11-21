use std::{
	net::{
		ToSocketAddrs,
		UdpSocket,
	},
	time::Duration,
};

use anyhow::Context;

pub fn factorio_check() -> anyhow::Result<bool> {
	let msg = [0x22, 0x00, 0x00, 0x02, 0x00, 0x07, 0x19, 0x36, 0x01, 0x00, 0xb3, 0x24, 0x43, 0x66];
	let socket = UdpSocket::bind("0.0.0.0:0")?;

	let factorio_server_addr = std::env::var("FACTORIO_ADDR")?;
	let addr = (factorio_server_addr, 34197u16)
		.to_socket_addrs()
		.unwrap()
		.next()
		.context("socket addr not found")?;
	socket.send_to(&msg, addr)?;
	let mut buf = [0; 64];
	socket.set_read_timeout(Some(Duration::from_secs(240)))?;
	match socket.recv(&mut buf) {
		Ok(_) => Ok(true),
		Err(_) => Ok(false),
	}
}
