use std::net::{IpAddr, UdpSocket};

use crate::{BridgeError, error::BridgeError::Protocol};

pub fn routed_local_ip() -> Result<IpAddr, BridgeError> {
    let socket =
        UdpSocket::bind("0.0.0.0:0").map_err(|_| protocol("local ICE socket setup failed"))?;
    socket
        .connect("8.8.8.8:80")
        .map_err(|_| protocol("no route is available for Ring media"))?;
    socket
        .local_addr()
        .map(|address| address.ip())
        .map_err(|_| protocol("local ICE address is unavailable"))
}

fn protocol(message: &str) -> BridgeError {
    Protocol(message.into())
}
